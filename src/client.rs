use std::error::Error;
use futures::stream::{BoxStream, SplitSink, SplitStream};
use prost::Message;
use tokio_tungstenite::{
    connect_async,
    tungstenite::http::Request,
    MaybeTlsStream,
    WebSocketStream,
};

use crate::client;
use futures::StreamExt;
use tokio::net::TcpStream;
use url::Url;
use base64::{Engine as _, engine::general_purpose::STANDARD, DecodeError};

use serde::Deserialize;
use serde_json::json;
use tokio_tungstenite::tungstenite::handshake::client::Response;
use client::clicks::OwnershipState;
use crate::coordinates::TileCoordinatesMap;

pub mod clicks {
    include!(concat!(env!("OUT_DIR"), "/clicks.v1.rs"));
}

#[derive(Deserialize)]
struct OwnershipResponse {
    data: String,  // base64 encoded protobuf
}

pub struct ClickPlanetRestClient {
    base_url: String,
}

pub const CLIENT_NAME: &'static str = "clickplanet client owned by valdo404";

impl ClickPlanetRestClient {
    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.to_string(),
        }
    }

    pub async fn click_tile(&self, tile_id: i32, country_id: &str) -> Result<(), Box<dyn std::error::Error>> {
        let request = client::clicks::ClickRequest {
            tile_id,
            country_id: country_id.to_string(),
        };

        let mut proto_bytes = Vec::new();
        request.encode(&mut proto_bytes)?;

        let client = reqwest::Client::new();

        let response = client
            .post(format!("https://{}/api/click", self.base_url))
            .header("User-Agent", CLIENT_NAME)
            .header("Content-Type", "application/json")
            .header("Origin", format!("https://{}", self.base_url))
            .header("Referer", format!("https://{}/", self.base_url))
            .json(&json!({
            "data": proto_bytes,
        }))
            .send()
            .await?;

        response.error_for_status()?;

        Ok(())
    }

    pub async fn get_ownerships_by_batch(
        &self,
        start_tile_id: i32,
        end_tile_id: i32,
    ) -> Result<client::clicks::OwnershipState, Box<dyn std::error::Error + Send + Sync>> {
        let client = reqwest::Client::new();

        let batch_request = client::clicks::BatchRequest {
            start_tile_id,
            end_tile_id,
        };

        let mut proto_bytes = Vec::new();
        batch_request.encode(&mut proto_bytes)
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)
            ?;

        let payload = json!({
            "data": proto_bytes,
        });

        let response = client
            .post(format!("https://{}/api/ownerships-by-batch", self.base_url))
            .header("User-Agent", CLIENT_NAME)
            .header("Content-Type", "application/json")
            .header("Origin", format!("https://{}", self.base_url))
            .header("Referer", format!("https://{}/", self.base_url))
            .json(&payload)
            .send()
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;

        let response_json: serde_json::Value = response.json().await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;
        let encoded_data = response_json["data"]
            .as_str()
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Invalid or missing data field in response"
                )
            });

        let str_result = response_json["data"]
            .as_str()
            .ok_or_else(|| std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Invalid or missing data field in response"
            ))
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;

        let result: Result<Vec<u8>, DecodeError> = STANDARD.decode(
            str_result
        );

        let proto_bytes: Vec<u8> = result.map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;

        let ownership_state = client::clicks::OwnershipState::decode(&proto_bytes[..])
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;

        Ok(ownership_state)
    }

    pub async fn get_ownerships(
        &self,
        index_coordinates: &TileCoordinatesMap,
    ) -> Result<client::clicks::OwnershipState, Box<dyn std::error::Error + Send + Sync>> {
        const BATCH_SIZE: i32 = 10000;

        let max_tile_id = (index_coordinates.len() as i32) - 1;

        let mut final_state = client::clicks::OwnershipState {
            ownerships: Vec::new(),
        };

        let mut start_tile_id = 1;
        while start_tile_id <= max_tile_id {
            let end_tile_id = (start_tile_id + BATCH_SIZE).min(max_tile_id);

            let result: Result<OwnershipState, Box<dyn Error + Send + Sync>> = self.get_ownerships_by_batch(start_tile_id, end_tile_id).await;

            match result {
                Ok(batch_state) => {
                    final_state.ownerships.extend(batch_state.ownerships);
                },
                Err(e) => {
                    eprintln!("Error fetching batch {} to {}: {:?}", start_tile_id, end_tile_id, e);

                    if let Some(reqwest_err) = e.downcast_ref::<reqwest::Error>() {
                        if let Some(status) = reqwest_err.status() {
                            eprintln!("HTTP Status: {}", status);
                            if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                                eprintln!("Rate limit hit, waiting before retry...");
                                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                                continue; // Retry this batch
                            }
                        }
                        if reqwest_err.is_timeout() {
                            eprintln!("Request timed out");
                        }
                        if reqwest_err.is_connect() {
                            eprintln!("Connection error");
                        }
                    }

                    return Err(e);
                }
            }

            start_tile_id += BATCH_SIZE;
        }

        Ok(final_state)
    }



    pub async fn connect_websocket(&self) -> Result<SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>, Box<dyn std::error::Error + Send + Sync>> {
        let ws_url = format!("wss://{}/ws/listen", self.base_url);
        let url = Url::parse(&ws_url).map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;

        let request = Request::builder()
            .uri(url.as_str())
            .header("User-Agent", CLIENT_NAME)
            .header("Origin", format!("https://{}", self.base_url))
            .header("Host", self.base_url.clone())  // Clone here
            .header("Connection", "Upgrade")
            .header("Upgrade", "websocket")
            .header("Sec-WebSocket-Version", "13")
            .header("Sec-WebSocket-Key", generate_websocket_key())
            .body(())
            .unwrap();

        let (ws_stream, _): (WebSocketStream<MaybeTlsStream<TcpStream>>, Response) = connect_async(request).await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;
        let (_, read): (SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, tokio_tungstenite::tungstenite::Message>, SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>) = ws_stream.split();
        Ok(read)
    }

    pub async fn listen_for_updates(&self) -> Result<BoxStream<'static, client::clicks::UpdateNotification>, Box<dyn std::error::Error + Send + Sync>> {
        let read = self.connect_websocket().await?;

        let update_stream = read
            .filter_map(|message| async move {
                match message {
                    Ok(msg) => {
                        client::clicks::UpdateNotification::decode(msg.into_data().as_slice()).ok()
                    },
                    Err(_) => None,
                }
            })
            .boxed();

        Ok(update_stream)
    }
}

pub fn generate_websocket_key() -> String {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use rand::Rng;

    let mut rng = rand::thread_rng();
    let mut key = [0u8; 16];
    rng.fill(&mut key);
    STANDARD.encode(key)
}