// src/bin/tor_ws.rs
use arti_client::{DataStream, TorClient, TorClientConfig};
use tor_rtcompat::PreferredRuntime;

use futures::StreamExt;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, RootCertStore, SignatureScheme};
use std::error::Error;
use tokio_tungstenite::tungstenite::handshake::client::{Request, Response};
use tokio_tungstenite::{client_async_tls_with_config, tungstenite, MaybeTlsStream, WebSocketStream};
use url::Url;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Initialize Tor client
    let config: TorClientConfig = TorClientConfig::default();

    let tor_client: TorClient<PreferredRuntime> = TorClient::create_bootstrapped(config).await?;

    // Set up Tor as a proxy for WebSocket connection
    let tor_stream: DataStream = tor_client.connect(("clickplanet.lol", 443)).await?;

    let ws_url = format!("wss://{}/ws/listen", "clickplanet.lol");
    let url = Url::parse(&ws_url)?;

    println!("WS Url: {}", ws_url);
    println!("Origin: {}", format!("https://{}", "clickplanet.lol"));


    let request = Request::builder()
        .uri(url.as_str())
        .header("User-Agent", "Test Tor")
        .header("Origin", format!("https://{}", "clickplanet.lol"))
        .header("Host", "clickplanet.lol".clone())  // Clone here
        .header("Connection", "Upgrade")
        .header("Upgrade", "websocket")
        .header("Sec-WebSocket-Version", "13")
        .header("Sec-WebSocket-Key", generate_websocket_key())
        .body(())
        .unwrap();

    // Establish WebSocket connection over the Tor stream
    let (ws_stream, _) = connect_over_tor(tor_stream, request).await?;

    // Now you can send and receive WebSocket messages
    let (_, mut read) = ws_stream.split();

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

#[derive(Debug)]
struct AcceptAllCerts;

impl ServerCertVerifier for AcceptAllCerts {
    fn verify_server_cert(&self, end_entity: &CertificateDer<'_>, intermediates: &[CertificateDer<'_>], server_name: &ServerName<'_>, ocsp_response: &[u8], now: UnixTime) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(&self, message: &[u8], cert: &CertificateDer<'_>, dss: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(&self, message: &[u8], cert: &CertificateDer<'_>, dss: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![]
    }
}

async fn connect_over_tor(
    tor_stream: DataStream,
    request: Request
) -> Result<(WebSocketStream<MaybeTlsStream<DataStream>>, Response), tungstenite::error::Error> {
    let mut root_store = RootCertStore::empty();
    root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());

    client_async_tls_with_config(request, tor_stream, None, None).await
}

