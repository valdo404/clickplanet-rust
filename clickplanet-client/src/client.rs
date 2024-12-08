use futures::stream::{BoxStream, SplitStream};
use prost::Message;
use std::error::Error;
use std::sync::Arc;
use std::time::Duration;
use tokio_tungstenite::{
    connect_async,
    tungstenite::http::Request,
    MaybeTlsStream,
    WebSocketStream,
};

use base64::{engine::general_purpose::STANDARD, DecodeError, Engine as _};
use clickplanet_proto::clicks;
use clickplanet_proto::clicks::OwnershipState;
use clickplanet_proto::clicks::*;
use futures::StreamExt;
use rand::Rng;
use serde::Deserialize;
use serde_json::json;
use tokio::net::TcpStream;
use tokio::time::sleep;
use tokio_retry::strategy::{jitter, ExponentialBackoff};
use tokio_retry::Retry;
use url::Url;

pub trait TileCount {
    fn len(&self) -> usize;

    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[derive(Clone)]
pub struct WebSocketConfig {
    initial_interval: Duration,
    max_interval: Duration
}

impl Default for WebSocketConfig {
    fn default() -> Self {
        Self {
            initial_interval: Duration::from_secs(1),
            max_interval: Duration::from_secs(60)
        }
    }
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
        let request = clicks::ClickRequest {
            tile_id,
            country_id: country_id.to_string(),
        };

        let mut proto_bytes = Vec::new();
        request.encode(&mut proto_bytes)?;

        let client = reqwest::Client::new();
        let base_url = self.base_url.clone();

        // Configure the retry strategy
        let retry_strategy = ExponentialBackoff::from_millis(100)
            .max_delay(Duration::from_secs(5))
            .take(2)
            .map(jitter);

        let result = Retry::spawn(retry_strategy, || async {
            let response = client
                .post(format!("https://{}/api/click", base_url))
                .header("User-Agent", CLIENT_NAME)
                .header("Content-Type", "application/json")
                .header("Origin", format!("https://{}", base_url))
                .header("Referer", format!("https://{}/", base_url))
                .json(&json!({
                "data": proto_bytes,
            }))
                .send()
                .await?;

            response.error_for_status().map(|_| ())
        }).await;

        match result {
            Ok(_) => Ok(()),
            Err(e) => Err(Box::new(e)),
        }
    }

    pub async fn get_ownerships_by_batch(
        &self,
        start_tile_id: i32,
        end_tile_id: i32,
    ) -> Result<clicks::OwnershipState, Box<dyn std::error::Error + Send + Sync>> {
        let client = reqwest::Client::new();

        let batch_request = clicks::BatchRequest {
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

        let ownership_state = clicks::OwnershipState::decode(&proto_bytes[..])
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;

        Ok(ownership_state)
    }

    pub async fn get_ownerships(
        &self,
        index_coordinates: &Arc<dyn TileCount + Send + Sync>,
    ) -> Result<clicks::OwnershipState, Box<dyn std::error::Error + Send + Sync>> {
        const BATCH_SIZE: i32 = 10000;

        let max_tile_id = (index_coordinates.len() as i32) - 1;

        let mut final_state = clicks::OwnershipState {
            ownerships: Vec::new(),
        };

        let mut start_tile_id = 1;
        while start_tile_id <= max_tile_id {
            let end_tile_id = (start_tile_id + BATCH_SIZE).min(max_tile_id);

            let millis = rand::thread_rng().gen_range(300..=1000);
            sleep(Duration::from_millis(millis)).await;

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
        let config = WebSocketConfig::default();

        let retry_strategy = ExponentialBackoff::from_millis(config.initial_interval.as_millis() as u64)
            .max_delay(config.max_interval)
            .map(jitter);

        let result = Retry::spawn(retry_strategy, || async {
            let ws_url = format!("wss://{}/ws/listen", self.base_url);
            let url = Url::parse(&ws_url).map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;

            let request = Request::builder()
                .uri(url.as_str())
                .header("User-Agent", CLIENT_NAME)
                .header("Origin", format!("https://{}", self.base_url))
                .header("Host", self.base_url.clone())
                .header("Connection", "Upgrade")
                .header("Upgrade", "websocket")
                .header("Sec-WebSocket-Version", "13")
                .header("Sec-WebSocket-Key", generate_websocket_key())
                .body(())?;

            println!("Attempting WebSocket connection...");
            let (ws_stream, _) = connect_async(request).await
                .map_err(|e| {
                    println!("Connection attempt failed: {:?}", e);
                    Box::new(e) as Box<dyn std::error::Error + Send + Sync>
                })?;

            println!("Successfully connected to WebSocket");

            Ok::<WebSocketStream<MaybeTlsStream<TcpStream>>, Box<dyn std::error::Error + Send + Sync>>(ws_stream)
        }).await?;

        let (_, read) = result.split();
        Ok(read)
    }

    pub async fn listen_for_updates(&self) -> Result<BoxStream<'_, clicks::UpdateNotification>, Box<dyn std::error::Error + Send + Sync + 'static>> {
        let stream = Box::pin(futures::stream::unfold((), move |_| {
            let client = self;  // No need to clone here
            async move {
                loop {
                    match Self::create_update_stream(client).await {
                        Ok(stream) => return Some((stream, ())),
                        Err(e) => {
                            eprintln!("Error in WebSocket connection: {}. Retrying...", e);
                            tokio::time::sleep(Duration::from_secs(1)).await;
                        }
                    }
                }
            }
        })
            .flat_map(|stream| stream));

        Ok(stream.boxed())
    }

    async fn create_update_stream<'a>(client: &'a ClickPlanetRestClient) -> Result<BoxStream<'a, clicks::UpdateNotification>, Box<dyn std::error::Error + Send + Sync + 'a>> {
        // Rest of the implementation remains the same
        let config = WebSocketConfig::default();
        let retry_strategy = ExponentialBackoff::from_millis(config.initial_interval.as_millis() as u64)
            .max_delay(config.max_interval)
            .map(jitter);

        let read = Retry::spawn(retry_strategy, || async {
            client.connect_websocket().await
        }).await?;

        let stream = read
            .filter_map(|message| async move {
                match message {
                    Ok(msg) => {
                        match clicks::UpdateNotification::decode(msg.into_data().as_slice()) {
                            Ok(notification) => Some(notification),
                            Err(e) => {
                                eprintln!("Error decoding message: {}", e);
                                None
                            }
                        }
                    },
                    Err(e) => {
                        eprintln!("WebSocket message error: {}", e);
                        None
                    }
                }
            })
            .boxed();

        Ok(stream)
    }
}

pub fn generate_websocket_key() -> String {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    use rand::Rng;

    let mut rng = rand::thread_rng();
    let mut key = [0u8; 16];
    rng.fill(&mut key);
    STANDARD.encode(key)
}