mod click_service;
mod nats_commons;
mod telemetry;
mod redis_click_persistence;
mod ownership_service;
mod click_persistence;
mod in_memory_click_persistence;

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
use tracing::{error};
use base64::{encode};
use clap::Parser;
use futures_util::{SinkExt, StreamExt};
use std::{time::Duration};
use axum::extract::WebSocketUpgrade;
use axum::http::header::CONTENT_TYPE;
use axum::http::Method;
use axum::serve::Serve;
use prost::Message;
use tokio::sync::Mutex;
use tokio::sync::broadcast;
use tokio::sync::broadcast::{Receiver, Sender};
use tower_http::cors::{Any, CorsLayer};
use clickplanet_proto::clicks::{Click, UpdateNotification};
use clickplanet_proto::clicks::{LeaderboardResponse, LeaderboardEntry};

use crate::click_persistence::{ClickRepository, LeaderboardRepository, LeaderboardOnClicks, LeaderboardMaintainer};
use crate::in_memory_click_persistence::{PapayaClickRepository};
use crate::nats_commons::ConsumerConfig;
use crate::ownership_service::OwnershipUpdateService;
use crate::redis_click_persistence::{RedisClickRepository};
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
struct AppState<T: ClickRepository + Send + Sync> {
    click_service: Arc<ClickService>,
    click_repository: Arc<T>,
    leaderboard_repo: Arc<dyn LeaderboardRepository>,
    update_notifification_broadcaster: Arc<Sender<UpdateNotification>>,
    ownership_update_service: Arc<OwnershipUpdateService>,
}


#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(long, env = "NATS_URL", default_value = "nats://localhost:4222")]
    nats_url: String,

    #[arg(long, env = "REDIS_URL", default_value = "redis://localhost:6379")]
    redis_url: String,

    #[arg(long, env = "OTEL_EXPORTER_OTLP_ENDPOINT", default_value = "http://localhost:4317")]
    otlp_endpoint: String,

    #[arg(long, env = "SERVICE_NAME", default_value = "clickplanet-server")]
    service_name: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    let telemetry_config = TelemetryConfig {
        otlp_endpoint: args.otlp_endpoint,
        service_name: args.service_name,
    };

    init_telemetry(telemetry_config).await?;

    run(&args.nats_url, &args.redis_url).await?;

    Ok(())
}

async fn run(nats_url: &str, redis_url: &str) -> Result<(), Box<dyn std::error::Error>> {
    let (click_sender, _) = broadcast::channel(100000);
    let click_sender_ref = Arc::new(click_sender);

    let (update_notification_sender, _) = broadcast::channel(100000);
    let update_sender_ref: Arc<Sender<UpdateNotification>> = Arc::new(update_notification_sender);

    let cold_repository: Arc<RedisClickRepository> = Arc::new(RedisClickRepository::new(redis_url).await?);
    let papaya_honey = PapayaClickRepository::populate_with(cold_repository).await?;

    let leaderboard_repo: Arc<dyn LeaderboardRepository> = Arc::new(LeaderboardOnClicks(papaya_honey.clone()));
    let click_repository: Arc<PapayaClickRepository> = Arc::new(papaya_honey.clone());

    let jetstream = Arc::new(get_or_create_jet_stream(nats_url).await?);

    let update_service = Arc::new(OwnershipUpdateService::new(
        click_repository.clone(),
        click_repository.clone(),
        click_sender_ref.clone(),
        update_sender_ref.clone(),
        jetstream.clone(),
        Some(ConsumerConfig {
            concurrent_processors: 2,
            ack_wait: Duration::from_secs(20),
            ..Default::default()
        })
    ));

    let state = AppState {
        click_service: Arc::new(ClickService::new(jetstream.clone(), click_sender_ref.clone()).await.unwrap()),
        click_repository: click_repository.clone(),
        leaderboard_repo: leaderboard_repo.clone(),
        update_notifification_broadcaster: update_sender_ref.clone(),
        ownership_update_service: update_service.clone(),
    };

    let app = Router::new()
        .route("/api/click", post(handle_click))
        .route("/v2/rpc/click", post(handle_click))
        .route("/api/ownerships-by-batch", post(handle_get_ownerships_by_batch))
        .route("/v2/rpc/ownerships-by-batch", post(handle_get_ownerships_by_batch))
        .route("/v2/rpc/ownerships", get(handle_get_ownerships))
        .route("/v2/rpc/leaderboard", get(handle_get_leaderboard))
        .route("/ws/listen", get(handle_ws_upgrade))
        .route("/v2/ws/listen", get(handle_ws_upgrade))
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
                .allow_headers([CONTENT_TYPE])
                .allow_credentials(true)
        )
        .with_state(state);

    let listener = TcpListener::bind("0.0.0.0:3000").await?;
    println!("Server listening on 0.0.0.0:3000");

    let server: Serve<Router, Router> = axum::serve(listener, app);
    let update_service_clone = update_service.clone();
    let update_service_handle = update_service_clone.run();

    tokio::select! {
        result = server => {
            if let Err(e) = result {
                error!("Server error: {:?}", e);
                return Err(e.to_string().into());
            } else {
                error!("Unexpected server exit");
            }
        }
        result = update_service_handle => {
            if let Err(e) = result {
               error!("Ownership update service error: {:?}", e);
               return Err(e.to_string().into());
            } else {
               error!("Unexpected update service exit");
            }
        }
    }

    Ok(())
}

async fn handle_click<T: ClickRepository>(
    State(state): State<AppState<T>>,
    Json(payload): Json<ClickPayload>,
) -> Result<impl IntoResponse, StatusCode> {
    let click_request = clickplanet_proto::clicks::ClickRequest::decode(Bytes::from(payload.data))
        .map_err(|_| {
            StatusCode::BAD_REQUEST
        })?;

    tokio::time::timeout(
        Duration::from_secs(10),
        state.click_service.process_click(click_request)
    )
        .await
        .map_err(|e| {
            error!("Timeout error while clicking: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .map_err(|e| {
            error!("Error while processing click: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;


    Ok(axum::Json(serde_json::json!({})))
}

async fn handle_get_ownerships<T: ClickRepository>(
    State(state): State<AppState<T>>,
) -> Result<Json<Value>, StatusCode> {
    let response = tokio::time::timeout(
        Duration::from_secs(5),
        state.click_repository.get_ownerships(),
    )
        .await
        .map_err(|e| {
            error!("Timeout error while calling get_ownerships: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .map_err(|e| {
            error!("Error while processing get_ownerships: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

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

async fn handle_get_ownerships_by_batch<T: ClickRepository>(
    State(state): State<AppState<T>>,
    Json(payload): Json<BatchRequestPayload>,
) -> Result<Json<Value>, StatusCode> {
    let batch_request = clickplanet_proto::clicks::BatchRequest::decode(Bytes::from(payload.data))
        .map_err(|_| StatusCode::BAD_REQUEST)?;

    let response = tokio::time::timeout(
        Duration::from_secs(5),
        state.click_repository.get_ownerships_by_batch(
            batch_request.start_tile_id as u32,
            batch_request.end_tile_id as u32,
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


async fn handle_ws_upgrade<T: ClickRepository+ 'static>(
    ws: WebSocketUpgrade,
    State(state): State<AppState<T>>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_ws_connection(socket, state))
}

async fn handle_ws_connection<T: ClickRepository>(socket: WebSocket, state: AppState<T>) {
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

    tokio::select! {
        _ = &mut send_task => recv_task.abort(),
        _ = &mut recv_task => send_task.abort(),
    }
}

async fn handle_get_leaderboard<T: ClickRepository>(
    State(state): State<AppState<T>>,
) -> Result<Json<Value>, StatusCode> {
    let leaderboard_data = tokio::time::timeout(
        Duration::from_secs(5),
        state.leaderboard_repo.leaderboard(),
    )
        .await
        .map_err(|e| {
            error!("Timeout error while fetching leaderboard: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .map_err(|e| {
            error!("Error while fetching leaderboard: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let mut response = LeaderboardResponse {
        entries: Vec::new(),
    };

    // Convert HashMap to vec and sort by score in descending order
    let mut entries: Vec<_> = leaderboard_data
        .into_iter()
        .map(|(country_id, score)| LeaderboardEntry {
            country_id,
            score,
        })
        .collect();

    entries.sort_by(|a, b| b.score.cmp(&a.score));
    response.entries = entries;

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