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
use axum::extract::WebSocketUpgrade;
use axum::http::header::CONTENT_TYPE;
use axum::http::{Method, Request};
use axum::serve::Serve;
use http_body_util::Full;
use axum::body::{to_bytes, Body};
use axum::http::header::ACCEPT;
use axum::response::Response;

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

use prost::Message;
use tokio::sync::Mutex;
use tokio::sync::broadcast;
use tokio::sync::broadcast::{Receiver, Sender};
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use clickplanet_proto::clicks::{Click, UpdateNotification};
use clickplanet_proto::clicks::{LeaderboardResponse, LeaderboardEntry};
use bytes::BytesMut;


use crate::click_persistence::{ClickRepository, LeaderboardRepository, LeaderboardOnClicks, LeaderboardMaintainer};
use crate::in_memory_click_persistence::{PapayaClickRepository};
use crate::nats_commons::ConsumerConfig;
use crate::ownership_service::OwnershipUpdateService;
use crate::redis_click_persistence::{RedisClickRepository};
use crate::telemetry::{init_telemetry, TelemetryConfig};



#[derive(Debug)]
enum AcceptedFormat {
    Protobuf,
    Json,
}

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


#[derive(Parser, Debug, Clone)]
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

    #[arg(long, env = "PORT", default_value = "3000")]
    port: u16,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    init_telemetry(TelemetryConfig {
        otlp_endpoint: args.otlp_endpoint.clone(),
        service_name: args.service_name.clone(),
    }).await?;

    run(&args).await?;

    Ok(())
}

async fn run(args: &Args) -> Result<(), Box<dyn std::error::Error>> {
    let (click_sender, _) = broadcast::channel(100000);
    let click_sender_ref = Arc::new(click_sender);

    let (update_notification_sender, _) = broadcast::channel(100000);
    let update_sender_ref: Arc<Sender<UpdateNotification>> = Arc::new(update_notification_sender);

    let cold_repository: Arc<RedisClickRepository> = Arc::new(RedisClickRepository::new(args.redis_url.as_str()).await?);
    let papaya_honey = PapayaClickRepository::populate_with(cold_repository).await?;

    let leaderboard_repo: Arc<dyn LeaderboardRepository> = Arc::new(LeaderboardOnClicks(papaya_honey.clone()));
    let click_repository: Arc<PapayaClickRepository> = Arc::new(papaya_honey.clone());

    let jetstream = Arc::new(get_or_create_jet_stream(args.nats_url.as_str()).await?);

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
        .route("/api/ownerships-by-batch", post(handle_get_ownerships_by_batch))
        .route("/ws/listen", get(handle_ws_upgrade))
        .route("/v2/rpc/click", post(handle_click))
        .route("/v2/rpc/ownerships-by-batch", post(handle_get_ownerships_by_batch))
        .route("/v2/rpc/ownerships", get(handle_get_ownerships))
        .route("/v2/rpc/leaderboard", get(handle_get_leaderboard))
        .route("/v2/ws/listen", get(handle_ws_upgrade))
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
                .allow_headers([CONTENT_TYPE])
        )
        .layer(TraceLayer::new_for_http()
            .make_span_with(|request: &Request<_>| {
                tracing::info_span!(
                    "http-request",
                    method = ?request.method(),
                    uri = ?request.uri(),
                    version = ?request.version(),
                )
            })
        )
        .with_state(state);

    let listener = TcpListener::bind(format!("0.0.0.0:{}", args.port)).await?;
    println!("Server listening on 0.0.0.0:{}", args.port);

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
    headers: axum::http::HeaderMap,
) -> Result<Response<Full<Bytes>>, StatusCode> {
    let format = match headers.get(ACCEPT) {
        Some(accept) if accept.to_str().map(|s| s.contains("application/protobuf")).unwrap_or(false) => {
            AcceptedFormat::Protobuf
        },
        _ => AcceptedFormat::Json
    };

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

    match format {
        AcceptedFormat::Protobuf => {
            let mut buf = BytesMut::new();
            response.encode(&mut buf)
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

            Ok(Response::builder()
                .header("Content-Type", "application/protobuf")
                .body(Full::new(buf.freeze()))
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?)
        },
        AcceptedFormat::Json => {
            let json_response = json!({
                "entries": response.entries.iter().map(|entry| {
                    json!({
                        "country_id": entry.country_id,
                        "score": entry.score,
                    })
                }).collect::<Vec<_>>()
            });

            Ok(Response::builder()
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(json_response.to_string())))
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?)
        }
    }
}