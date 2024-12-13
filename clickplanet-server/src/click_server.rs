mod click_service;
mod nats_commons;
mod telemetry;
mod redis_click_persistence;
mod ownership_service;

use crate::click_service::{get_or_create_jet_stream, ClickService};
use axum::{
    extract::{Json, State},
    extract::ws::{Message as WebsocketMessage},
    extract::ws::{WebSocket},
    http::StatusCode,
    response::IntoResponse,
    routing::post,
    routing::get,
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

use futures_util::{SinkExt, StreamExt};
use std::{time::Duration};
use axum::extract::WebSocketUpgrade;
use axum::serve::Serve;
use prost::Message;
use tokio::sync::Mutex;
use tokio::sync::broadcast;
use tokio::sync::broadcast::{Receiver, Sender};
use tokio::task::JoinHandle;
use clickplanet_proto::clicks::{Click, UpdateNotification};
use crate::nats_commons::ConsumerConfig;
use crate::ownership_service::OwnershipUpdateService;
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
    update_notifification_broadcaster: Arc<Sender<UpdateNotification>>,
    ownership_update_service: Arc<OwnershipUpdateService>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_telemetry(TelemetryConfig::default()).await?;

    run("nats://localhost:4222", "redis://localhost:6379").await?;

    Ok(())
}

async fn run(nats_url: &str, redis_url: &str) -> Result<(), Box<dyn std::error::Error>> {
    let (click_sender, _) = broadcast::channel(1000);
    let click_sender_ref = Arc::new(click_sender);

    let (update_notification_sender, _) = broadcast::channel(1000);
    let update_sender_ref = Arc::new(update_notification_sender);

    let repository = Arc::new(RedisClickRepository::new(redis_url).await.unwrap());

    let jetstream = Arc::new(get_or_create_jet_stream(nats_url).await?);

    let update_service = Arc::new(OwnershipUpdateService::new(
        repository.clone(),
        click_sender_ref.clone(),
        update_sender_ref.clone(),
        jetstream.clone(),
        Some(ConsumerConfig {
            concurrent_processors: 8,
            ack_wait: Duration::from_secs(10),
            ..Default::default()
        })
    ));

    let state = AppState {
        click_service: Arc::new(ClickService::new(jetstream.clone(), click_sender_ref.clone()).await.unwrap()),
        click_persistence: repository.clone(),
        update_notifification_broadcaster: update_sender_ref.clone(),
        ownership_update_service: update_service.clone(),
    };

    let app = Router::new()
        .route("/api/click", post(handle_click))
        .route("/api/ownerships-by-batch", post(handle_get_ownerships_by_batch))
        .route("/v2/rpc/ownerships-by-batch", post(handle_get_ownerships_by_batch))
        .route("/ws/listen", get(handle_ws_upgrade))
        .route("/v2/ws/listen", get(handle_ws_upgrade))
        .with_state(state);

    let listener = TcpListener::bind("0.0.0.0:3000").await?;
    println!("Server listening on 0.0.0.0:3000");

    let server: Serve<Router, Router> = axum::serve(listener, app);
    let update_service_handle = update_service.run();

    tokio::select! {
        result = server => {
            if let Err(e) = result {
                error!("Server error: {:?}", e);
                return Err(e.to_string().into());
            }
        }
        result = update_service_handle => {
            if let Err(e) = result {
                error!("Ownership update service error: {:?}", e);
                return Err(e.to_string().into());
            }
        }
    }

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
        Duration::from_secs(5),
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
        Duration::from_secs(5),
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

    let mut update_notification_subscription: Receiver<UpdateNotification> = state.update_notifification_broadcaster.subscribe();
    let sender_arc_clone = sender_arc.clone();

    let mut send_task = tokio::spawn(async move {
        while let Ok(notification) = update_notification_subscription.recv().await {
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