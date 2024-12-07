// src/bin/tor_ws.rs
use arti_client::{DataStream, TorClient, TorClientConfig};
use tor_rtcompat::PreferredRuntime;

use tokio_tungstenite::{
    connect_async,
    tungstenite::http::Request,
    MaybeTlsStream,
    WebSocketStream,
};

use futures::SinkExt;
use futures::{Stream, StreamExt, TryStreamExt};
use std::error::Error;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::handshake::client::Response;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Initialize Tor client
    let config: TorClientConfig = TorClientConfig::default();

    let tor_client: TorClient<PreferredRuntime> = TorClient::create_bootstrapped(config).await?;

    // Set up Tor as a proxy for WebSocket connection
    let proxy_stream: DataStream = tor_client.connect(("clickplanet.lol", 443)).await?;

    // Establish WebSocket connection over the Tor stream
    // let (ws_stream, _) = connect_async(proxy_stream).await?;

    // Now you can send and receive WebSocket messages
    // let (mut write, mut read) = ws_stream.split();

    // Send a message
    // write.send("Hello from Tor WebSocket!".into()).await?;

    // Receive a message
    // if let Some(msg) = read.next().await {
     //   println!("Received: {:?}", msg?);
    //}

    Ok(())
}
