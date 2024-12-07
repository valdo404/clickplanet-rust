use futures::stream::{BoxStream, SplitSink, SplitStream};
use prost::Message;
use tokio_tungstenite::{
    connect_async,
    tungstenite::http::Request,
    MaybeTlsStream,
    WebSocketStream,
};

use crate::client;
use futures::{Stream, StreamExt, TryStreamExt};
use tokio::net::TcpStream;
use url::Url;
use base64::{Engine as _, engine::general_purpose::STANDARD};

use serde::Deserialize;
use serde_json::json;
use tokio_tungstenite::tungstenite::handshake::client::Response;

pub mod clicks {
    include!(concat!(env!("OUT_DIR"), "/clicks.v1.rs"));
}

#[derive(Deserialize)]
struct OwnershipResponse {
    data: String,  // base64 encoded protobuf
}

pub struct Client {
    base_url: String,
}

const CLIENT_NAME: &'static str = "clickplanet client owned by valdo404";

impl Client {
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
        // Implement HTTP POST request here
        Ok(())
    }

    pub async fn get_ownerships_by_batch(
        &self,
        start_tile_id: i32,
        end_tile_id: i32,
    ) -> Result<client::clicks::OwnershipState, Box<dyn std::error::Error>> {
        let client = reqwest::Client::new();

        // Create BatchRequest
        let batch_request = client::clicks::BatchRequest {
            start_tile_id,
            end_tile_id,
        };

        // Serialize BatchRequest to Protobuf bytes
        let mut proto_bytes = Vec::new();
        batch_request.encode(&mut proto_bytes)?;

        let payload = json!({
            "data": proto_bytes,
        });

        // Send request
        let response = client
            .post(format!("https://{}/api/ownerships-by-batch", self.base_url))
            .header("User-Agent", CLIENT_NAME)
            .header("Content-Type", "application/json")
            .header("Origin", format!("https://{}", self.base_url))
            .header("Referer", format!("https://{}/", self.base_url))
            .json(&payload)
            .send()
            .await?;

        // Parse JSON response
        let response_json: serde_json::Value = response.json().await?;
        let encoded_data = response_json["data"]
            .as_str()
            .ok_or("Invalid or missing data field in response")?;

        // Decode base64
        let proto_bytes = STANDARD.decode(encoded_data)?;

        // Decode OwnershipState Protobuf
        let ownership_state = client::clicks::OwnershipState::decode(&proto_bytes[..])?;

        Ok(ownership_state)
    }

    pub async fn get_ownerships(&self) -> Result<client::clicks::OwnershipState, Box<dyn std::error::Error>> {
        let client = reqwest::Client::new();

        let response = client
            .get(format!("https://{}/api/ownerships", self.base_url))
            .header("User-Agent", CLIENT_NAME)
            .header("Origin", format!("https://{}", self.base_url))
            .header("Referer", format!("https://{}/", self.base_url))
            .send()
            .await?;

        // Parse JSON response
        let ownership_response: OwnershipResponse = response.json().await?;

        // Decode base64
        let proto_bytes = STANDARD.decode(ownership_response.data)?;

        // Decode protobuf
        let ownership_state = client::clicks::OwnershipState::decode(&proto_bytes[..])?;

        Ok(ownership_state)
    }

    fn generate_websocket_key() -> String {
        use base64::{Engine as _, engine::general_purpose::STANDARD};
        use rand::Rng;

        let mut rng = rand::thread_rng();
        let mut key = [0u8; 16];
        rng.fill(&mut key);
        STANDARD.encode(key)
    }

    pub async fn connect_websocket(&self) -> Result<SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>, Box<dyn std::error::Error>> {
        let ws_url = format!("wss://{}/ws/listen", self.base_url);
        let url = Url::parse(&ws_url)?;

        let request = Request::builder()
            .uri(url.as_str())
            .header("User-Agent", CLIENT_NAME)
            .header("Origin", format!("https://{}", self.base_url))
            .header("Host", self.base_url.clone())  // Clone here
            .header("Connection", "Upgrade")
            .header("Upgrade", "websocket")
            .header("Sec-WebSocket-Version", "13")
            .header("Sec-WebSocket-Key", Self::generate_websocket_key())
            .body(())
            .unwrap();

        let (ws_stream, _): (WebSocketStream<MaybeTlsStream<TcpStream>>, Response) = connect_async(request).await?;
        let (_, read): (SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, tokio_tungstenite::tungstenite::Message>, SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>) = ws_stream.split();
        Ok(read)
    }

    pub async fn listen_for_updates(&self) -> Result<BoxStream<'static, client::clicks::UpdateNotification>, Box<dyn std::error::Error>> {
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
