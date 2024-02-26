use std::collections::HashMap;
use std::net::SocketAddr;
use std::str::FromStr;
use std::time::Duration;

use http_body_util::Full;
use hyper::body::Bytes;
use hyper::Request;
use hyper::{server::conn::http1, service::service_fn, Response};
use hyper_util::rt::TokioIo;
use massa_api_exports::execution::Transfer;
use massa_models::address::Address;
use massa_models::amount::Amount;
use massa_models::slot::Slot;
use massa_sdk::{Client, ClientConfig, HttpConfig};
use massa_time::MassaTime;
use mysql::prelude::Queryable;
use mysql::Pool;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tokio::spawn(async move {
        let config = HttpConfig {
            client_config: ClientConfig {
                max_concurrent_requests: 10,
                max_request_body_size: 1000000,
                request_timeout: MassaTime::from_millis(1000),
                certificate_store: "Native".to_string(),
                id_kind: "Number".to_string(),
                max_log_length: 1000000,
                headers: vec![],
            },
            enabled: true,
        };
        let client = Client::new(
            "127.0.0.1".parse().unwrap(),
            33035,
            33036,
            33037,
            33038,
            77,
            &config,
        )
        .await
        .unwrap();
        let url = "mysql://user:password@localhost:3307/db";
        let pool = Pool::new(url).unwrap();

        let mut conn = pool.get_conn().unwrap();
        conn.query_drop(
            "CREATE TABLE IF NOT EXISTS transfers (
            id INT PRIMARY KEY NOT NULL AUTO_INCREMENT,
            slot varchar(100) not null,
            from_addr varchar(100) not null,
            to_addr varchar(100) not null,
            amount bigint not null,
            context text not null
        )",
        )
        .unwrap();
        conn.query_drop(
            "CREATE TABLE IF NOT EXISTS metadata (
            id INT PRIMARY KEY NOT NULL AUTO_INCREMENT,
            key_text text not null,
            value_text text not null
        )",
        )
        .unwrap();

        let mut last_saved_slot = match conn
            .query_first::<String, _>(
                "SELECT value_text FROM metadata WHERE key_text = 'last_slot'",
            )
            .unwrap()
        {
            Some(res) => {
                let parts: Vec<&str> = res.split('_').collect();
                Slot::new(parts[0].parse().unwrap(), parts[1].parse().unwrap())
            }
            None => {
                let get_status = client.public.get_status().await.unwrap();
                let last_slot_api = get_status.execution_stats.final_cursor;
                conn.exec_drop(
                    "INSERT INTO metadata (key_text, value_text) VALUES (?, ?)",
                    (
                        "last_slot",
                        format!("{}_{}", last_slot_api.period, last_slot_api.thread),
                    ),
                )
                .unwrap();
                last_slot_api
            }
        };
        loop {
            let get_status = client.public.get_status().await.unwrap();
            let range_end = get_status.execution_stats.final_cursor;
            let mut last_inserted_slot = last_saved_slot;
            let mut slots = vec![];
            while range_end != last_inserted_slot {
                last_inserted_slot = last_inserted_slot.get_next_slot(32).unwrap();
                slots.push(last_inserted_slot);
            }
            let slots_transfers = client
                .public
                .get_slots_transfers(slots.clone())
                .await
                .unwrap();
            for (currents, slot) in slots_transfers.iter().zip(slots.iter()) {
                for transfer in currents.iter() {
                    conn.exec_drop("INSERT INTO transfers (slot, from_addr, to_addr, amount, context) VALUES (?, ?, ?, ?, ?)", (
                        format!("{}_{}", slot.period, slot.thread),
                        transfer.from.to_string(),
                        transfer.to.to_string(),
                        transfer.amount.to_raw(),
                        serde_json::to_string(&transfer.context).unwrap()
                    )).unwrap();
                }
                last_saved_slot = *slot;
                conn.exec_drop(
                    "UPDATE metadata SET value_text = ? WHERE key_text = 'last_slot'",
                    (format!("{}_{}", slot.period, slot.thread),),
                )
                .unwrap();
            }
            // Save DB
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    });

    let addr = SocketAddr::from(([127, 0, 0, 1], 4444));

    // We create a TcpListener and bind it to 127.0.0.1:4444
    let listener = TcpListener::bind(addr).await?;

    // We start a loop to continuously accept incoming connections
    loop {
        let (stream, _) = listener.accept().await?;

        // Use an adapter to access something implementing `tokio::io` traits as if they implement
        // `hyper::rt` IO traits.
        let io = TokioIo::new(stream);

        // Spawn a tokio task to serve multiple connections concurrently
        tokio::task::spawn(async move {
            // Finally, we bind the incoming connection to our `hello` service
            if let Err(err) = http1::Builder::new()
                // `service_fn` converts our function in a `Service`
                .serve_connection(io, service_fn(transfers))
                .await
            {
                println!("Error serving connection: {:?}", err);
            }
        });
    }
}

async fn transfers(
    req: Request<hyper::body::Incoming>,
) -> Result<Response<Full<Bytes>>, hyper::http::Error> {
    let url = "mysql://user:password@localhost:3307/db";
    let pool = Pool::new(url).unwrap();

    let mut conn = pool.get_conn().unwrap();
    let params =
        form_urlencoded::parse(&req.uri().query().unwrap().as_bytes()).collect::<HashMap<_, _>>();
    let slot = params.get("slot");
    let Some(slot) = slot else {
        return Response::builder()
            .status(400)
            .body(Full::new(Bytes::from("Slot not found")));
    };
    let slot = slot.split('_').collect::<Vec<&str>>();
    if slot.len() != 2 {
        return Response::builder()
            .status(400)
            .body(Full::new(Bytes::from("Invalid slot")));
    }
    let Ok(period) = slot[0].parse::<u64>() else {
        return Response::builder()
            .status(400)
            .body(Full::new(Bytes::from("Invalid period")));
    };
    let Ok(thread) = slot[1].parse::<u8>() else {
        return Response::builder()
            .status(400)
            .body(Full::new(Bytes::from("Invalid thread")));
    };
    let slot = Slot::new(period, thread);
    let Ok(res) = conn.exec::<(String, String, u64, String), _, _>(
        "SELECT from_addr, to_addr, amount, context FROM transfers WHERE slot = ?",
        (format!("{}_{}", slot.period, slot.thread),),
    ) else {
        return Response::builder()
            .status(500)
            .body(Full::new(Bytes::from("Internal error")));
    };
    let mut transfers = Vec::new();
    for transfer in res {
        transfers.push(Transfer {
            from: Address::from_str(&transfer.0).unwrap(),
            to: Address::from_str(&transfer.1).unwrap(),
            amount: Amount::from_raw(transfer.2),
            context: serde_json::from_str(&transfer.3).unwrap(),
        });
    }
    Ok(Response::new(Full::new(Bytes::from(
        serde_json::to_string(&transfers).unwrap(),
    ))))
}
