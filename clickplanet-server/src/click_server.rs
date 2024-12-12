mod click_service;
mod constants;
mod telemetry;
mod redis_click_persistence;

use crate::click_service::ClickService;
use axum::{
    extract::{Json, State},
    extract::ws::{Message as WebsocketMessage},
    extract::ws::{self, WebSocket},
    http::StatusCode,
    response::IntoResponse,
    routing::post,
    routing::get,
    routing::any,
    Router,
};

use bytes::Bytes;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use serde_json::{json, Value};
use tokio;
use tokio::net::TcpListener;
use tracing::{error, info};
use base64::{encode};

use futures_util::{stream::SplitStream, SinkExt, StreamExt};
use tokio::sync::mpsc;
use std::{time::Duration};
use axum::extract::WebSocketUpgrade;
use prost::Message;
use tokio::sync::Mutex;
use clickplanet_proto::clicks;
use tokio::sync::broadcast;
use tokio::sync::broadcast::{Receiver, Sender};

use clickplanet_proto::clicks::UpdateNotification;
use crate::redis_click_persistence::RedisClickRepository;
use crate::telemetry::{init_telemetry, TelemetryConfig};

#[derive(Debug, Serialize, Deserialize)]
struct ClickPayload {
    data: Vec<u8>,
}

#[derive(Debug, Serialize, Deserialize)]
struct BatchRequestPayload {
    data: Vec<u8>,
}

#[derive(Clone)]
struct AppState {
    click_service: Arc<ClickService>,
    click_persistence: Arc<RedisClickRepository>,
    click_broadcaster: Arc<broadcast::Sender<clicks::Click>>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_telemetry(TelemetryConfig::default()).await?;

    run("nats://localhost:4222", "redis://localhost:6379").await?;

    Ok(())
}

async fn run(nats_url: &str, redis_url: &str) -> Result<(), Box<dyn std::error::Error>> {
    let (sender, _) = broadcast::channel(32);
    let sender_ref = Arc::new(sender);

    let state = AppState {
        click_service: Arc::new(ClickService::new(nats_url, sender_ref.clone()).await.unwrap()),
        click_persistence: Arc::new(RedisClickRepository::new(redis_url).await.unwrap()),
        click_broadcaster: sender_ref
    };

    let app = Router::new()
        .route("/api/click", post(handle_click))
        .route("/api/ownerships-by-batch", post(handle_get_ownerships_by_batch))
        .route("/ws/listen", get(handle_ws_upgrade))  // Add this
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

async fn handle_click(
    State(state): State<AppState>,
    Json(payload): Json<ClickPayload>,
) -> Result<impl IntoResponse, StatusCode> {
    let click_request = clickplanet_proto::clicks::ClickRequest::decode(Bytes::from(payload.data))
        .map_err(|_| {
            StatusCode::BAD_REQUEST
        })?;

    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        state.click_service.process_click(click_request)
    )
        .await
        .map_err(|e| {
            error!("Timeout error while calling get_ownerships_by_batch: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .map_err(|e| {
            error!("Error while processing click: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;


    Ok(axum::Json(serde_json::json!({})))
}

async fn handle_get_ownerships_by_batch(
    State(state): State<AppState>,
    Json(payload): Json<BatchRequestPayload>,
) -> Result<Json<Value>, StatusCode> {
    let batch_request = clickplanet_proto::clicks::BatchRequest::decode(Bytes::from(payload.data))
        .map_err(|_| StatusCode::BAD_REQUEST)?;

    info!("Request: {:?}", batch_request);

    let response = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        state.click_persistence.get_ownerships_by_batch(
            batch_request.start_tile_id,
            batch_request.end_tile_id,
        ),
    )
        .await
        .map_err(|e| {
            error!("Timeout error while calling get_ownerships_by_batch: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .map_err(|e| {
            error!("Error while processing get_ownerships_by_batch: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    info!("Response: {:?}", response);

    let mut response_bytes = Vec::new();

    response
        .encode(&mut response_bytes)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let base64_data = encode(&response_bytes);

    let payload = json!({
        "data": base64_data,
    });

    Ok(axum::Json(payload))
}


async fn handle_ws_upgrade(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_ws_connection(socket, state))
}
async fn handle_ws_connection(socket: WebSocket, state: AppState) {
    let (sender, mut receiver) = socket.split();
    let sender_arc = Arc::new(Mutex::new(sender));

    let mut click_subscription = state.click_broadcaster.subscribe();
    let sender_arc_clone = sender_arc.clone();

    let mut send_task = tokio::spawn(async move {
        while let Ok(notification) = click_subscription.recv().await {
            let mut buf = Vec::new();
            if notification.encode(&mut buf).is_ok() {
                let mut sender = sender_arc.lock().await;
                if let Err(e) = sender.send(WebsocketMessage::Binary(buf)).await {
                    eprintln!("Error sending WebSocket message: {}", e);
                    break;
                }
            }
        }
    });


    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(message)) = receiver.next().await {
            match message {
                WebsocketMessage::Ping(payload) => {
                    let mut sender = sender_arc_clone.lock().await;
                    if let Err(e) = sender.send(WebsocketMessage::Pong(payload)).await {
                        eprintln!("Error sending pong: {}", e);
                        break;
                    }
                }
                WebsocketMessage::Close(_) => break,
                _ => {}
            }
        }
    });

    // Wait for either task to complete
    tokio::select! {
        _ = &mut send_task => recv_task.abort(),
        _ = &mut recv_task => send_task.abort(),
    }
}