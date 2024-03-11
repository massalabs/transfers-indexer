use std::collections::HashMap;
use std::net::SocketAddr;
use std::str::FromStr;
use std::time::Duration;

use dotenv::dotenv;
use http_body_util::Full;
use hyper::body::Bytes;
use hyper::header::CONTENT_TYPE;
use hyper::Request;
use hyper::{server::conn::http1, service::service_fn, Response};
use hyper_util::rt::TokioIo;
use massa_api_exports::execution::TransferContext;
use massa_models::address::Address;
use massa_models::amount::Amount;
use massa_models::block_id::BlockId;
use massa_models::slot::Slot;
use massa_models::timeslots::get_block_slot_timestamp;
use massa_sdk::{Client, ClientConfig, HttpConfig};
use massa_time::MassaTime;
use mysql::prelude::Queryable;
use mysql::Pool;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use time::{format_description, PrimitiveDateTime};
use tokio::net::TcpListener;
use prometheus::{register_int_gauge, Encoder, IntGauge, TextEncoder};
use lazy_static::lazy_static;

lazy_static! {
    static ref LAST_SLOT_PERIOD: IntGauge =
        register_int_gauge!("last_slot_period", "period of the slot of the last operation saved in database").unwrap();
    static ref LAST_SLOT_THREAD: IntGauge =
        register_int_gauge!("last_slot_thread", "thread of the slot of the last operation saved in database").unwrap();
}

pub fn set_last_slot_period(period: u64) {
    LAST_SLOT_PERIOD.set(period as i64);
}

pub fn set_last_slot_thread(thread: u8) {
    LAST_SLOT_THREAD.set(thread as i64);
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    dotenv().ok();
    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default_panic(info);
        std::process::exit(1);
    }));

    let url = std::env::var("DATABASE_URL").unwrap();
    let pool = Pool::new(url.as_str()).unwrap();
    let mut conn = pool.get_conn().unwrap();

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
            std::env::var("MASSA_NODE_API_IP").unwrap().parse().unwrap(),
            std::env::var("MASSA_NODE_API_PUBLIC_PORT")
                .unwrap()
                .parse()
                .unwrap(),
            33036,
            33037,
            33038,
            77658377,
            &config,
        )
        .await
        .unwrap();

        conn.query_drop(
            "CREATE TABLE IF NOT EXISTS transfers (
            id INT PRIMARY KEY NOT NULL AUTO_INCREMENT,
            slot_timestamp datetime not null,
            slot varchar(100) not null,
            from_addr varchar(100) not null,
            to_addr varchar(100) not null,
            amount bigint not null,
            effective_amount_received bigint not null,
            block_id varchar(100) not null,
            fee bigint not null,
            succeed int not null,
            context text not null,
            operation_id varchar(100)
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
        let _ = conn.query_drop(
            "CREATE INDEX idx_slot_timestamp ON transfers(slot_timestamp);
            CREATE INDEX idx_from_addr ON transfers(from_addr);
            CREATE INDEX idx_to_addr ON transfers(to_addr);
            CREATE INDEX idx_block_id ON transfers(block_id);
            CREATE INDEX idx_operation_id ON transfers(operation_id);"
        );
        let save_only_success = bool::from_str(&std::env::var("SAVE_ONLY_SUCCESS_TRANSFERS").unwrap_or("false".to_string())).unwrap();
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
                if slots.len() == 20 {
                    break;
                }
            }
            let slots_transfers = client
                .public
                .get_slots_transfers(slots.clone())
                .await
                .unwrap();
            if slots_transfers.is_empty() && !slots.is_empty() {
                last_saved_slot = slots.last().unwrap().clone();
                conn.exec_drop(
                    "UPDATE metadata SET value_text = ? WHERE key_text = 'last_slot'",
                    (format!(
                        "{}_{}",
                        last_saved_slot.period, last_saved_slot.thread
                    ),),
                )
                .unwrap();
                continue;
            }
            for (currents, slot) in slots_transfers.iter().zip(slots.iter()) {
                let slot_timestamp = get_block_slot_timestamp(
                    get_status.config.thread_count,
                    get_status.config.t0,
                    get_status.config.genesis_timestamp,
                    *slot,
                )
                .unwrap();
                for transfer in currents.iter() {
                    if save_only_success && !transfer.succeed {
                        continue;
                    }
                    match transfer.context {
                        TransferContext::Operation(operation_id) => {
                            conn.exec_drop(
                                "INSERT INTO transfers (slot, slot_timestamp, from_addr, to_addr, amount, effective_amount_received, block_id, fee, succeed, context, operation_id) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                                (
                                    format!("{}_{}", slot.period, slot.thread),
                                    slot_timestamp.format_instant().trim_end_matches('Z'),
                                    transfer.from.to_string(),
                                    transfer.to.to_string(),
                                    transfer.amount.to_raw(),
                                    transfer.effective_amount_received.to_raw(),
                                    transfer.block_id.to_string(),
                                    transfer.fee.to_raw(),
                                    transfer.succeed,
                                    serde_json::to_string(&transfer.context).unwrap(),
                                    operation_id.to_string(),
                                )
                            ).unwrap();
                        }
                        TransferContext::ASC(_index) => {
                            conn.exec_drop(
                                "INSERT INTO transfers (slot, slot_timestamp, from_addr, to_addr, block_id, fee, succeed, amount, effective_amount_received, context) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                                (
                                    format!("{}_{}", slot.period, slot.thread),
                                    slot_timestamp.format_instant().trim_end_matches('Z'),
                                    transfer.from.to_string(),
                                    transfer.to.to_string(),
                                    transfer.block_id.to_string(),
                                    transfer.fee.to_raw(),
                                    transfer.succeed,
                                    transfer.amount.to_raw(),
                                    transfer.effective_amount_received.to_raw(),
                                    serde_json::to_string(&transfer.context).unwrap()
                                )
                            ).unwrap();
                        }
                    }
                }
                last_saved_slot = *slot;
                conn.exec_drop(
                    "UPDATE metadata SET value_text = ? WHERE key_text = 'last_slot'",
                    (format!("{}_{}", slot.period, slot.thread),),
                )
                .unwrap();

                set_last_slot_period(slot.period);
                set_last_slot_thread(slot.thread);        
            }
            // Save DB
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    });
    
    let addr = std::env::var("INDEXER_API")
        .unwrap()
        .parse::<SocketAddr>()
        .unwrap();

    // We create a TcpListener and bind it to 127.0.0.1:4444
    let listener = TcpListener::bind(addr).await?;

    // We start a loop to continuously accept incoming connections
    loop {
        let (stream, _) = listener.accept().await?;

        // Use an adapter to access something implementing `tokio::io` traits as if they implement
        // `hyper::rt` IO traits.
        let io = TokioIo::new(stream);
        let pool = pool.clone();
        let make_svc = service_fn(move |req| indexer_api(req, pool.clone()));

        // Spawn a tokio task to serve multiple connections concurrently
        tokio::task::spawn(async move {
            // Finally, we bind the incoming connection to our `hello` service
            if let Err(err) = http1::Builder::new()
                // `service_fn` converts our function in a `Service`
                .serve_connection(io, make_svc)
                .await
            {
                println!("Error serving connection: {:?}", err);
            }
        });
    }
}

async fn indexer_api(
    req: Request<hyper::body::Incoming>,
    pool: Pool,
) -> Result<Response<Full<Bytes>>, hyper::http::Error> {
    let mut conn = pool.get_conn().unwrap();

    match req.uri().path() {
        "/transfers" => {
            if req.method() != hyper::Method::GET {
                return Response::builder()
                    .status(405)
                    .body(Full::new(Bytes::from("Method not allowed")));
            }
            let params = form_urlencoded::parse(req.uri().query().unwrap_or_default().as_bytes())
            .collect::<HashMap<_, _>>();
    
        let mut conditions = vec![];
        match params.get("from") {
            Some(from) => {
                let Ok(from_addr) = Address::from_str(from) else {
                    return Response::builder()
                        .status(400)
                        .body(Full::new(Bytes::from("Invalid from address")));
                };
                conditions.push(format!("from_addr = '{}' ", from_addr));
            }
            None => {}
        }
    
        match params.get("to") {
            Some(to) => {
                let Ok(to_addr) = Address::from_str(to) else {
                    return Response::builder()
                        .status(400)
                        .body(Full::new(Bytes::from("Invalid to address")));
                };
                conditions.push(format!("to_addr = '{}' ", to_addr));
            }
            None => {}
        }
        match params.get("operation_id") {
            Some(operation_id) => {
                conditions.push(format!("operation_id = '{}' ", operation_id));
            }
            None => {}
        }
        match params.get("succeed") {
            Some(succeed) => {
                conditions.push(format!("succeed = {} ", succeed));
            }
            None => {}
        }
        match params.get("start_date") {
            Some(start_date) => {
                conditions.push(format!(
                    "slot_timestamp >= '{}' ",
                    start_date.trim_end_matches('Z')
                ));
            }
            None => {}
        }
        match params.get("end_date") {
            Some(end_date) => {
                conditions.push(format!(
                    "slot_timestamp <= '{}' ",
                    end_date.trim_end_matches('Z')
                ));
            }
            None => {}
        }
        if conditions.is_empty() {
            conditions.push("1 = 1".to_string());
        }
        let Ok(res) = conn.exec::<(String, String, String, u64, bool, u64, u64, String, PrimitiveDateTime ), _, _>(
            format!("SELECT from_addr, to_addr, block_id, fee, succeed, amount, effective_amount_received, context, slot_timestamp FROM transfers WHERE {}", conditions.join(" AND ")),
            (),
        ) else {
            return Response::builder()
                .status(500)
                .body(Full::new(Bytes::from("Internal error")));
        };
        let transfers = res.into_par_iter().map(|transfer| {
            TransferResponse {
                from: Address::from_str(&transfer.0).unwrap(),
                to: Address::from_str(&transfer.1).unwrap(),
                block_id: BlockId::from_str(&transfer.2).unwrap(),
                fee: Amount::from_raw(transfer.3),
                succeed: transfer.4,
                amount: Amount::from_raw(transfer.5),
                effective_amount_received: Amount::from_raw(transfer.6),
                context: serde_json::from_str(&transfer.7).unwrap(),
                operation_time: transfer
                    .8
                    .format(&format_description::well_known::Iso8601::DATE_TIME)
                    .unwrap(),
            }
        }).collect::<Vec<TransferResponse>>();
        Ok(Response::new(Full::new(Bytes::from(
            serde_json::to_string(&transfers).unwrap(),
        ))))
        },
        "/last_slot" => {
            let Ok(res) = conn.exec::<String,_,_>(
                format!("SELECT value_text FROM metadata WHERE key_text='last_slot'"),
                (),
            ) else {
                return Response::builder()
                    .status(500)
                    .body(Full::new(Bytes::from("Internal error")));
            };
            return Ok(Response::new(Full::new(Bytes::from(
                serde_json::to_string(&res).unwrap(),
            ))))
        },
        "/metrics" => {
            let encoder = TextEncoder::new();
            let mut buffer = vec![];
            encoder
                .encode(&prometheus::gather(), &mut buffer)
                .expect("Failed to encode metrics");
            let response = Response::builder()
                .status(200)
                .header(CONTENT_TYPE, encoder.format_type())
                .body(Full::new(Bytes::from(buffer)))
                .unwrap();
    
            Ok(response)
        }
        _ => {
            return Response::builder()
                .status(404)
                .body(Full::new(Bytes::from("Not found")));
        }
    }
}

#[derive(Debug, Deserialize, Clone, Serialize)]
pub struct TransferResponse {
    /// The sender of the transfer
    pub from: Address,
    /// The receiver of the transfer
    pub to: Address,
    /// The amount of the transfer
    pub amount: Amount,
    /// Effective amount received
    pub effective_amount_received: Amount,
    /// If the transfer succeed or not
    pub succeed: bool,
    /// Fee
    pub fee: Amount,
    /// Block ID
    pub block_id: BlockId,
    /// operation time
    pub operation_time: String,
    /// Context
    pub context: TransferContext,
}
