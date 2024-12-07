use arti_client::{DataStream, TorClient, TorClientConfig};
use tor_rtcompat::PreferredRuntime;

use crate::client::CLIENT_NAME;
use futures::StreamExt;
use rustls::client::danger::ServerCertVerifier;
use std::error::Error;
use tokio_tungstenite::tungstenite::handshake::client::{Request, Response};
use tokio_tungstenite::{client_async_tls_with_config, tungstenite, MaybeTlsStream, WebSocketStream};
use url::Url;

mod client;

const HOST: &'static str = "clickplanet.lol";

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let config: TorClientConfig = TorClientConfig::default();
    let tor_client: TorClient<PreferredRuntime> = TorClient::create_bootstrapped(config).await?;

    let tor_stream: DataStream = tor_client.connect((HOST, 443)).await?;

    let ws_url = format!("wss://{}/ws/listen", HOST);
    let url = Url::parse(&ws_url)?;

    println!("WS Url: {}", ws_url);
    println!("Origin: {}", format!("https://{}", HOST));

    let request = Request::builder()
        .uri(url.as_str())
        .header("User-Agent", CLIENT_NAME)
        .header("Origin", format!("https://{}", HOST))
        .header("Host", HOST.clone())  // Clone here
        .header("Connection", "Upgrade")
        .header("Upgrade", "websocket")
        .header("Sec-WebSocket-Version", "13")
        .header("Sec-WebSocket-Key", generate_websocket_key())
        .body(())
        .unwrap();

    let (ws_stream, _) = connect_over_tor(tor_stream, request).await?;

    let (_, mut read) = ws_stream.split();

    // will fail because of cloudflare, and http gets too

    if let Some(msg) = read.next().await {
        println!("Received: {:?}", msg?);
    }

    Ok(())
}

fn generate_websocket_key() -> String {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    use rand::Rng;

    let mut rng = rand::thread_rng();
    let mut key = [0u8; 16];
    rng.fill(&mut key);
    STANDARD.encode(key)
}

async fn connect_over_tor(
    tor_stream: DataStream,
    request: Request
) -> Result<(WebSocketStream<MaybeTlsStream<DataStream>>, Response), tungstenite::error::Error> {
    client_async_tls_with_config(request, tor_stream, None, None).await
}

