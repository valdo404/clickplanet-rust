mod click_service;

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

#[derive(Debug, Serialize, Deserialize)]
struct ClickPayload {
    data: Vec<u8>,
}

#[derive(Clone)]
struct AppState {
    click_service: Arc<ClickService>
}

#[tokio::main]
async fn main() {
    if let Err(err) = run().await {
        eprintln!("Error: {}", err);
        std::process::exit(1);
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let service = ClickService::new("nats://localhost:4222").await?;

    let state = AppState {
        click_service: Arc::new(service)
    };

    let app = Router::new()
        .route("/api/click", post(handle_click))
        .with_state(state);

    let listener = TcpListener::bind("0.0.0.0:3000").await?;
    println!("Server listening on 0.0.0.0:3000");

    axum::serve(listener, app).await?;

    Ok(())
}


async fn handle_click(
    State(state): State<AppState>,
    Json(payload): Json<ClickPayload>,
) -> Result<impl IntoResponse, StatusCode> {
    // Decode protobuf request
    let click_request = clickplanet_proto::clicks::ClickRequest::decode(Bytes::from(payload.data))
        .map_err(|_| StatusCode::BAD_REQUEST)?;

    // Process click using service
    let response = state.click_service
        .process_click(click_request)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Encode protobuf response
    let mut response_bytes = Vec::new();
    response
        .encode(&mut response_bytes)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Return encoded protobuf with correct content type
    Ok((
        [(axum::http::header::CONTENT_TYPE, "application/x-protobuf")],
        response_bytes,
    ))
}