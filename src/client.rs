use crate::clicks::{ClickRequest, OwnershipState};
use futures::stream::{BoxStream, SplitStream};
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

pub mod clicks {
    include!(concat!(env!("OUT_DIR"), "/clicks.v1.rs"));
}

pub struct Client {
    base_url: String,
}

impl Client {
    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.to_string(),
        }
    }

    pub async fn click_tile(&self, tile_id: i32, country_id: &str) -> Result<(), Box<dyn std::error::Error>> {
        let request = ClickRequest {
            tile_id,
            country_id: country_id.to_string(),
        };
        // Implement HTTP POST request here
        Ok(())
    }

    pub async fn get_ownerships(&self) -> Result<OwnershipState, Box<dyn std::error::Error>> {
        // Implement HTTP GET request here
        unimplemented!()
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
            .header("User-Agent", "Rust Client")
            .header("Origin", format!("https://{}", self.base_url))
            .header("Host", self.base_url.clone())  // Clone here
            .header("Connection", "Upgrade")
            .header("Upgrade", "websocket")
            .header("Sec-WebSocket-Version", "13")
            .header("Sec-WebSocket-Key", Self::generate_websocket_key())
            .body(())
            .unwrap();

        let (ws_stream, _) = connect_async(request).await?;
        let (_, read) = ws_stream.split();
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
