// click_service.rs

use async_nats::{jetstream, ConnectError};
use prost::Message;
use std::hash::{DefaultHasher, Hash, Hasher};
use thiserror::Error;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use uuid::Uuid;

const NUM_PARTITIONS: usize = 32;

pub struct ClickService {
    jetstream: jetstream::Context,
}

#[derive(Error, Debug)]
pub enum ClickServiceError {
    #[error("Failed to connect to NATS: {0}")]
    ConnectionError(#[from] ConnectError),  // Changed to specific ConnectError
    #[error("Failed to create stream: {0}")]
    StreamCreationError(String),
}


impl ClickService {
    pub async fn new(nats_url: &str) -> Result<Self, ClickServiceError> {
        let client = async_nats::connect(nats_url).await?;
        let jetstream = async_nats::jetstream::new(client);

        let stream_config = async_nats::jetstream::stream::Config {
            name: "CLICKS".to_string(),
            subjects: vec!["clicks.partition.*".to_string()],
            max_age: Duration::from_secs(8 * 60 * 60),
            discard: async_nats::jetstream::stream::DiscardPolicy::Old,
            ..Default::default()
        };

        match jetstream.get_stream("CLICKS").await {
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

        Ok(Self { jetstream })
    }

    pub async fn process_click(
        &self,
        request: clickplanet_proto::clicks::ClickRequest,
    ) -> Result<clickplanet_proto::clicks::ClickResponse, Box<dyn std::error::Error + Send + Sync>> {
        let click_id = Uuid::new_v4();
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;

        let partition = Self::consistent_hash(request.tile_id, NUM_PARTITIONS);
        let subject = format!("clicks.partition.{}", partition);

        let response = clickplanet_proto::clicks::ClickResponse {
            timestamp_ns: timestamp,
            click_id: click_id.to_string(),
        };

        let click_data = clickplanet_proto::clicks::Click {
            tile_id: request.tile_id,
            country_id: request.country_id,
            timestamp_ns: timestamp,
            click_id: click_id.to_string(),
        };

        let mut click_bytes = Vec::new();
        click_data.encode(&mut click_bytes)?;

        self.jetstream
            .publish(subject, click_bytes.into())
            .await?;

        Ok(response)
    }

    fn consistent_hash<T: Hash>(key: T, num_partitions: usize) -> usize {
        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        (hasher.finish() as usize) % num_partitions
    }
}