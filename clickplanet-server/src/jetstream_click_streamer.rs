use async_nats::jetstream::consumer::{Config, Consumer};
use async_nats::jetstream::stream::Stream;
use async_nats::jetstream::Context;
use async_nats::{jetstream, ConnectError};
use clickplanet_proto::clicks::{Click, ClickResponse};
use futures::stream::{self};
use futures::{future, StreamExt};
use prost::Message;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use thiserror::Error;
use tracing::{error, info, instrument, Span};
use crate::redis_click_persistence::{RedisClickRepository, RedisPersistenceError};
use crate::constants;
use crate::constants::CLICK_STREAM_NAME;

#[derive(Clone, Debug)]
pub struct ConsumerConfig {
    pub consumer_name: String,
    pub ack_wait: Duration,
    pub max_deliver: i64,
    pub concurrent_processors: usize,
}

impl Default for ConsumerConfig {
    fn default() -> Self {
        Self {
            consumer_name: "tile-state-processor".to_string(),
            ack_wait: Duration::from_secs(30),
            max_deliver: 3,
            concurrent_processors: 4,
        }
    }
}


#[derive(Error, Debug)]
pub enum PollingConsumerError {
    #[error("Failed to connect to NATS: {0}")]
    NatsConnection(#[from] ConnectError),
    #[error("Failed to process message: {0}")]
    Processing(String),
    #[error("Failed to decode protobuf: {0}")]
    ProtobufDecode(#[from] prost::DecodeError),
    #[error("Failed to decode protobuf: {0}")]
    RedisPersistence(#[from] RedisPersistenceError)
}

const CONSUMER_NAME: &'static str = "tile-state-processor";

pub struct ClickConsumer {
    jetstream: Arc<jetstream::Context>,
    consumer_config: ConsumerConfig,
    redis_click_repository: Arc<RedisClickRepository>,
}

impl ClickConsumer {
    pub async fn new(nats_url: &str, consumer_config: Option<ConsumerConfig>, redis_click_repository: RedisClickRepository) -> Result<Self, PollingConsumerError> {
        let client = async_nats::connect(nats_url).await?;
        let jetstream = async_nats::jetstream::new(client);

        Ok(Self {
            jetstream: Arc::new(jetstream),
            consumer_config: consumer_config.unwrap_or_default(),
            redis_click_repository: Arc::new(redis_click_repository)
        })
    }

    pub async fn create_consumer(
        &self,
    ) -> Result<jetstream::consumer::pull::Stream, PollingConsumerError> {
        let stream = get_stream(self.jetstream.clone()).await?;

        let config = jetstream::consumer::pull::Config {
            durable_name: Some(CONSUMER_NAME.to_string()),
            deliver_policy: jetstream::consumer::DeliverPolicy::All,
            ack_policy: jetstream::consumer::AckPolicy::Explicit,
            ack_wait: self.consumer_config.ack_wait,
            max_deliver: self.consumer_config.max_deliver,
            ..Default::default()
        };

        let consumer = stream
            .create_consumer(config)
            .await
            .map_err(|e| PollingConsumerError::Processing(e.to_string()))?;

        let messages = consumer
            .messages()
            .await
            .map_err(|e| PollingConsumerError::Processing(e.to_string()))?;

        Ok(messages)
    }

    pub async fn run(&self) -> Result<(), PollingConsumerError> {
        let consumer = self.create_consumer().await?;
        info!("Starting stream processor");

        consumer
            .map(|message_result| {
                let this = self.clone();

                async move {
                    match message_result {
                        Ok(msg) => {
                            info!("Processing message on subject: {}", msg.subject);
                            if let Err(e) = this.handle_message(msg).await {
                                error!("Error processing message: {}", e);
                            }
                        }
                        Err(e) => error!("Error receiving message: {}", e),
                    }
                }
            })
            .buffer_unordered(self.consumer_config.concurrent_processors)
            .for_each(|_| future::ready(()))
            .await;

        Ok(())
    }

    async fn handle_message(&self, message: jetstream::Message) -> Result<(), PollingConsumerError> {
        let subject = message.subject.as_str();

        let tile_id: i32 = subject
            .strip_prefix(constants::CLICK_SUBJECT_PREFIX)
            .and_then(|id| id.parse().ok())
            .ok_or_else(|| PollingConsumerError::Processing("Invalid subject format".to_string()))?;
        let click: Click = clickplanet_proto::clicks::Click::decode(message.payload.clone())?;

        self.redis_click_repository.save_click(tile_id, &click).await?;

        message
            .ack()
            .await
            .map_err(|e| PollingConsumerError::Processing(e.to_string()))?;

        Ok(())
    }
}


pub async fn get_stream(jetstream: Arc<Context>) -> Result<jetstream::stream::Stream, PollingConsumerError> {
    jetstream.get_stream(CLICK_STREAM_NAME.to_string())
        .await
        .map_err(|e| PollingConsumerError::Processing(e.to_string()))
}