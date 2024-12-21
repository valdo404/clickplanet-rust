use base64::Engine;
use futures_util::stream::StreamExt;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::backends::backend::{Ownerships, OwnershipsGetter, TileClicker, Update, UpdatesListener};
use anyhow::Result;
use base64::engine::general_purpose;
use clickplanet_proto::clicks::{BatchRequest, ClickRequest, OwnershipState};
use prost::Message;
use tokio::net::TcpStream;
use tokio::time::sleep;
use tokio_tungstenite::tungstenite::handshake::client::Response;
use tokio_tungstenite::tungstenite::protocol::Message as WsMessage;
use tokio_tungstenite::tungstenite::Error as WsError;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use uuid::Uuid;

#[derive(Clone)]
pub struct ClickServiceClient {
    pub config: Config,
}

impl ClickServiceClient {
    pub async fn fetch(&self, verb: &str, path: &str, body: Option<&[u8]>) -> Result<Option<Vec<u8>>> {
        let url = format!("{}{}", self.config.base_url, path);
        let config = self.config.timeout_ms;
        // retry loop
        for _ in 0..5 {
            let res = self.make_request(verb, &url, body, config).await;
            if let Ok(r) = res {
                return r;
            }
        }

        Err(anyhow::anyhow!("Failed to fetch after multiple retries"))
    }


    async fn make_request(&self, verb: &str, url: &str, body: Option<&[u8]>, timeout_ms: u32) -> Result<Result<Option<Vec<u8>>>> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(timeout_ms as u64))
            .build()?;
        let mut request = match verb {
            "POST" => client.post(url),
            _ => client.get(url),
        };
        if let Some(b) = body {
            request = request.body(b.to_vec());
        }


        let res = request.send().await?;
        if !res.status().is_success() {
            return Ok(Err(anyhow::anyhow!(
                "Failed to fetch: {} {}",
                res.status(),
                res.text().await?
            )));
        }
        let json: serde_json::Value = res.json().await?;

        let base64_string = json.get("data").and_then(|v| v.as_str());

        Ok(Ok(base64_string.map(|s| {
            general_purpose::STANDARD.decode(s).unwrap()
        })))
    }
}

#[derive(Clone)]
pub struct HTTPBackend {
    client: ClickServiceClient,
    pending_updates: Vec<Update>,
    update_batch_callbacks: HashMap<String, Arc<Mutex<Box<dyn Fn(Vec<Update>) + Send + Sync + 'static>>>>
}

impl HTTPBackend {

    pub fn new(client: ClickServiceClient, batch_update_duration_ms: u64) -> Self {

        let backend: HTTPBackend = Self {
            client,
            pending_updates: Vec::new(),
            update_batch_callbacks: HashMap::new(),
        };


        let mut backend_updates = backend.clone();
        tokio::spawn(async move {
            loop{
                if !backend_updates.pending_updates.is_empty(){

                    let updates = backend_updates.pending_updates.clone();
                    backend_updates.pending_updates.clear();
                    for callback in backend_updates.update_batch_callbacks.values() {
                        let callback = callback.lock().unwrap();
                        callback(updates.clone());
                    }

                }
                sleep(Duration::from_millis(batch_update_duration_ms)).await;
            }
        });



        backend
    }


}

impl TileClicker for HTTPBackend {
    fn click_tile(&mut self, tile_id: u32, country_id: String) -> () {
        let payload = ClickRequest {
            tile_id: tile_id.try_into().unwrap(),
            country_id,
        };
        let mut buf = Vec::new();

        payload.encode(&mut buf).unwrap();
        // dbg!(&buf);
        let _ = self.client.fetch("POST", "/v2/rpc/click", Some(buf.as_slice()));
    }
}

impl OwnershipsGetter for HTTPBackend {
    fn get_current_ownerships_by_batch(
        &self,
        batch_size: usize,
        max_index: usize,
        callback: Box<dyn Fn(Ownerships) + Send + Sync>,
    ) {
        for i in (1..max_index).step_by(batch_size) {
            let payload = BatchRequest {
                end_tile_id: (i + batch_size) as i32,
                start_tile_id: i as i32,
            };
            let mut buf = Vec::new();

            payload.encode(&mut buf).unwrap();

            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();

            let binary = runtime
                .block_on(self.client.fetch("POST", "/v2/rpc/ownerships-by-batch", Some(buf.as_slice()))).unwrap().unwrap();

            let message = OwnershipState::decode(binary.as_slice()).unwrap();

            let bindings_map: HashMap<_, _> = message
                .ownerships
                .into_iter()
                .map(|ownership| (ownership.tile_id, ownership.country_id))
                .collect();

            callback(Ownerships { bindings: bindings_map });
        }
    }
}


impl UpdatesListener for HTTPBackend {
    fn listen_for_updates(&self, _callback: Box<dyn Fn(Update) + Send + Sync>) {
        let protocol = if self.client.config.base_url.starts_with("https") {
            "wss"
        } else {
            "ws"
        };

        let host = self
            .client
            .config
            .base_url
            .replace("https://", "")
            .replace("http://", "");

        let url = format!("{}://{}/v2/ws/listen", protocol, host);

        let config = self.client.config.timeout_ms;

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        let ws_stream = runtime.block_on(init_websocket(&url, config))
            .unwrap();

        let mut receive_stream = ws_stream.map(|msg| -> Result<WsMessage, WsError> {
            match msg {
                Ok(m) => Ok(m),
                Err(e) => Err(e),
            }

        });

        loop {

            let _next = receive_stream.next();

            // let message = TileUpdate::decode(message.as_slice()).unwrap();

            // let update = Update {
            //     tile: message.tile_id,
            //     previous_country: {
            //         if message.previous_country_id.is_empty() {
            //             None
            //         }
            //         else {
            //             Some(message.previous_country_id)
            //         }
            //
            //     },
            //     new_country: message.country_id,
            // };
            //
            // _callback(update);

        }

    }

    fn listen_for_updates_batch(
        &mut self,
        callback: Box<dyn Fn(Vec<Update>) + Send + Sync>,
    ) -> () {
        let id = Uuid::new_v4().to_string();
        self.update_batch_callbacks.insert(id, Arc::new(Mutex::new(callback)));
        // Closure to remove the callback
    }
}


async fn init_websocket(url: &str, _timeout_ms: u32) -> Result<WebSocketStream<MaybeTlsStream<TcpStream>>> {
    let (ws_stream, _): (WebSocketStream<MaybeTlsStream<TcpStream>>, Response) = tokio_tungstenite::connect_async(url).await.unwrap();

    Ok(ws_stream)
}

#[derive(Clone)]
pub struct Config {
    pub base_url: String,
    pub timeout_ms: u32,
}
