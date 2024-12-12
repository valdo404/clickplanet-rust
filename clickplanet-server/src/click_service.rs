use async_nats::{jetstream, ConnectError};
use prost::Message;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use async_nats::jetstream::Context;
use thiserror::Error;
use tokio::sync::broadcast::Sender;
use tracing::{field, info, instrument, Instrument, Span};
use uuid::Uuid;
use clickplanet_proto::clicks::{Click, UpdateNotification};
use crate::constants;
use crate::constants::{CLICK_STREAM_NAME, CLICK_SUBJECT_PREFIX};

pub struct ClickService {
    jetstream: jetstream::Context,
    sender: Arc<Sender<Click>>,
}

#[derive(Error, Debug)]
pub enum ClickServiceError {
    #[error("Failed to connect to NATS: {0}")]
    ConnectionError(#[from] ConnectError),  // Changed to specific ConnectError
    #[error("Failed to create stream: {0}")]
    StreamCreationError(String),
}

pub async fn get_or_create_jet_stream(nats_url: &str) -> Result<Context, ClickServiceError> {
    let client = async_nats::connect(nats_url).await?;
    let jetstream = async_nats::jetstream::new(client);

    let stream_config = async_nats::jetstream::stream::Config {
        name: CLICK_STREAM_NAME.to_string(),
        subjects: vec![format!("{}*", CLICK_SUBJECT_PREFIX).to_string()],
        max_age: Duration::from_secs(8 * 60 * 60),
        discard: async_nats::jetstream::stream::DiscardPolicy::Old,
        ..Default::default()
    };

    match jetstream.get_stream(CLICK_STREAM_NAME).await {
        Ok(_) => {
            jetstream
                .update_stream(stream_config)
                .await
                .map_err(|e| ClickServiceError::StreamCreationError(e.to_string()))?;
        }
        Err(_) => {
            jetstream
                .create_stream(stream_config)
                .await
                .map_err(|e| ClickServiceError::StreamCreationError(e.to_string()))?;
        }
    }

    Ok(jetstream)
}


impl ClickService {
    pub async fn new(nats_url: &str, sender: Arc<Sender<Click>>) -> Result<Self, ClickServiceError> {
        let jetstream = get_or_create_jet_stream(nats_url).await?;

        Ok(Self { jetstream, sender })
    }

    #[instrument(
        name = "process_click",
        skip(self, request),
        fields(
        tile_id = tracing::field::Empty,
        country = tracing::field::Empty,
        timestamp = tracing::field::Empty,
        click_id = tracing::field::Empty,
        publish_time = tracing::field::Empty,
        )
    )]
    pub async fn process_click(
        &self,
        request: clickplanet_proto::clicks::ClickRequest,
    ) -> Result<clickplanet_proto::clicks::ClickResponse, Box<dyn std::error::Error + Send + Sync>> {

        let click_id = Uuid::new_v4();
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;

        let subject = format!("{}{}", CLICK_SUBJECT_PREFIX, request.tile_id);

        let response = clickplanet_proto::clicks::ClickResponse {
            timestamp_ns: timestamp,
            click_id: click_id.to_string(),
        };

        let click_data = clickplanet_proto::clicks::Click {
            tile_id: request.tile_id,
            country_id: request.country_id.clone(),
            timestamp_ns: timestamp,
            click_id: click_id.to_string(),
        };

        let mut click_bytes = Vec::new();
        click_data.encode(&mut click_bytes)?;

        self.jetstream
            .publish(subject, click_bytes.into())
            .await?;

        self.sender.send(click_data)?;

        let publish_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;

        // Record span values after async operations
        let span = Span::current();
        span.record("tile_id", request.tile_id);
        span.record("country", &request.country_id);
        span.record("timestamp", timestamp);
        span.record("click_id", &click_id.to_string());
        span.record("publish_time", publish_time);

        info!(
        "Click processed successfully for tile {} (country: {})",
            request.tile_id, request.country_id
        );

        Ok(response)
    }
}