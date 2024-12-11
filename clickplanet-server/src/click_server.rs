mod click_service;
mod constants;
mod telemetry;

use crate::click_service::ClickService;
use axum::{
    extract::{Json, State},
    http::StatusCode,
    response::IntoResponse,
    routing::post,
    Router,

};
use bytes::Bytes;
use prost::Message;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio;
use tokio::net::TcpListener;
use tracing::info;
use crate::telemetry::{init_telemetry, TelemetryConfig};

#[derive(Debug, Serialize, Deserialize)]
struct ClickPayload {
    data: Vec<u8>,
}

#[derive(Clone)]
struct AppState {
    click_service: Arc<ClickService>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_telemetry(TelemetryConfig::default()).await?;

    run("nats://localhost:4222").await?;

    Ok(())
}

async fn run(nats_url: &str) -> Result<(), Box<dyn std::error::Error>> {
    let state = AppState {
        click_service: Arc::new(ClickService::new(nats_url).await.unwrap())
    };

    let app = Router::new()
        .route("/api/click", post(handle_click))
        .with_state(state);

    let listener = TcpListener::bind("0.0.0.0:3000").await?;
    println!("Server listening on 0.0.0.0:3000");

    // Keep telemetry context alive by wrapping the server
    let server = axum::serve(listener, app);

    // Run the server inside a tracing span
    tracing::info_span!("server").in_scope(|| async {
        server.await
    }).await?;

    Ok(())
}

#[axum::debug_handler]
async fn handle_click(
    State(state): State<AppState>,
    Json(payload): Json<ClickPayload>,
) -> Result<impl IntoResponse, StatusCode> {
    let click_request = clickplanet_proto::clicks::ClickRequest::decode(Bytes::from(payload.data))
        .map_err(|_| {
            StatusCode::BAD_REQUEST
        })?;

    let response = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        state.click_service.process_click(click_request)
    )
        .await
        .map_err(|_| {
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .map_err(|_| {
            StatusCode::INTERNAL_SERVER_ERROR
        })?;


    let mut response_bytes = Vec::new();
    response
        .encode(&mut response_bytes)
        .map_err(|_| {
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok((
        [(axum::http::header::CONTENT_TYPE, "application/x-protobuf")],
        response_bytes,
    ))
}