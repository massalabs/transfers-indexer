use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::Duration;

use http_body_util::Full;
use hyper::body::Bytes;
use hyper::Request;
use hyper::{server::conn::http1, service::service_fn, Response};
use hyper_util::rt::TokioIo;
use massa_models::slot::{Slot, SlotSerializer};
use massa_sdk::{Client, ClientConfig, HttpConfig};
use massa_serialization::Serializer;
use massa_time::MassaTime;
use rocksdb::DB;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let db = DB::open_default("./db").unwrap();
    let slot_ser = SlotSerializer::new();
    let last_saved_slot: Option<Slot> = None;

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
        loop {
            let range_start = last_saved_slot.unwrap_or(Slot::new(0, 0));
            let get_status = client.public.get_status().await.unwrap();
            let range_end = get_status.last_slot.unwrap();
            // TODO: generate slots from range_start to range_end
            let slots: Vec<Slot> = vec![range_start, range_end];
            let slots_transfers = client
                .public
                .get_slots_transfers(slots.clone())
                .await
                .unwrap();
            for (currents, slot) in slots_transfers.iter().zip(slots.iter()) {
                let mut raw_slot = vec![];
                slot_ser.serialize(&slot, &mut raw_slot).unwrap();
                for (index, transfer) in currents.iter().enumerate() {
                    let mut raw_transfer: Vec<u8> = vec![];
                    // TODO: implement transfer serializer
                    let key = [raw_slot.clone(), index.to_be_bytes().to_vec()].concat();
                    // TODO: save raw transfer to DB
                    let _ = db.put(key, []);
                }
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
    println!("Slot: {:?}", slot);
    // TODO: pass DB instance to struct with this as member function
    // TODO: fetch transfers from DB with slot prefix iterator
    // TODO: implement transfer deserializer
    Ok(Response::new(Full::new(Bytes::from("Hello, World!"))))
}
