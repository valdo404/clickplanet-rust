use async_nats::jetstream::consumer::{Config, Consumer};
use async_nats::jetstream::stream::Stream;
use async_nats::jetstream::Context;
use async_nats::{jetstream, ConnectError};
use deadpool_redis::{redis::cmd, Config as RedisConfig, PoolError, Runtime};
use futures::{future, StreamExt};
use prost::Message;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use thiserror::Error;
use tracing::{error, info, instrument, Span};
use crate::constants;
use crate::constants::CLICK_STREAM_NAME;

const REDIS_KEY_PREFIX: &str = "tile:";

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

#[derive(Clone)]
pub struct RedisTileStateBuilder {
    jetstream: Arc<jetstream::Context>,
    redis_pool: Arc<deadpool_redis::Pool>,
    consumer_config: ConsumerConfig,
}

#[derive(Error, Debug)]
pub enum PollingConsumerError {
    #[error("Failed to connect to NATS: {0}")]
    NatsConnection(#[from] ConnectError),
    #[error("Failed to connect to Redis: {0}")]
    RedisConnection(#[from] deadpool_redis::CreatePoolError),
    #[error("Failed to process message: {0}")]
    Processing(String),
    #[error("Failed to decode protobuf: {0}")]
    ProtobufDecode(#[from] prost::DecodeError),
    #[error("Redis error: {0}")]
    Redis(#[from] deadpool_redis::redis::RedisError),
    #[error("Redis pool error: {0}")]
    RedisPool(#[from] PoolError),
}

const CONSUMER_NAME: &'static str = "tile-state-processor";

impl RedisTileStateBuilder {
    pub async fn new(nats_url: &str, redis_url: &str, consumer_config: Option<ConsumerConfig>,
    ) -> Result<Self, PollingConsumerError> {
        let client = async_nats::connect(nats_url).await?;
        let jetstream = async_nats::jetstream::new(client);

        let redis_cfg = RedisConfig::from_url(redis_url);
        let redis_pool = redis_cfg.create_pool(Some(Runtime::Tokio1))?;

        Ok(Self {
            jetstream: Arc::new(jetstream),
            redis_pool: Arc::new(redis_pool),
            consumer_config: consumer_config.unwrap_or_default(),
        })
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

    async fn create_consumer(
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

    pub async fn get_leaderboard(
        &self,
        top_n: usize,
    ) -> Result<Vec<(String, f64)>, PollingConsumerError> {
        let leaderboard_key = format!("{}leaderboard", REDIS_KEY_PREFIX);
        let mut redis_conn = self.redis_pool.get().await?;

        let scores: Vec<(String, f64)> = cmd("ZREVRANGE")
            .arg(&leaderboard_key)
            .arg(0)
            .arg((top_n - 1) as isize)
            .arg("WITHSCORES")
            .query_async(&mut redis_conn)
            .await?;

        Ok(scores)
    }

    pub async fn get_tiles(
        &self,
        tile_id: i32,
    ) -> Result<(Option<String>, Option<u64>), PollingConsumerError> {
        let tile_key = format!("{}{}:country", REDIS_KEY_PREFIX, tile_id);
        let timestamp_key = format!("{}{}:timestamp", REDIS_KEY_PREFIX, tile_id);

        let mut redis_conn = self.redis_pool.get().await?;

        // Fetch both keys using a Redis pipeline
        let (country_id, timestamp): (Option<String>, Option<u64>) = deadpool_redis::redis::pipe()
            .atomic()
            .cmd("GET").arg(&tile_key) // Get country ID
            .cmd("GET").arg(&timestamp_key) // Get last modification timestamp
            .query_async(&mut redis_conn)
            .await?;

        Ok((country_id, timestamp))
    }

    #[instrument(
        name = "process_message",
        skip(self, message),
        fields(
           tile_id = tracing::field::Empty,
           country = tracing::field::Empty,
           message_timestamp = tracing::field::Empty,
           message_receive_time = tracing::field::Empty,
           message_processing_time = tracing::field::Empty,s
        )
    )]
    async fn handle_message(&self, message: jetstream::Message) -> Result<(), PollingConsumerError> {
        let subject = message.subject.as_str();

        let receive_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;

        let tile_id: i32 = subject
            .strip_prefix(constants::CLICK_SUBJECT_PREFIX)
            .and_then(|id| id.parse().ok())
            .ok_or_else(|| PollingConsumerError::Processing("Invalid subject format".to_string()))?;

        let click = clickplanet_proto::clicks::Click::decode(message.payload.clone())?;

        info!(
           "Received click for tile {} (country: {}, timestamp: {})",
           tile_id, click.country_id, click.timestamp_ns
        );

        let mut redis_conn = self.redis_pool.get().await?;

        let tile_key = format!("{}{}:country", REDIS_KEY_PREFIX, tile_id);
        let timestamp_key = format!("{}{}:timestamp", REDIS_KEY_PREFIX, tile_id);
        let leaderboard_key = format!("leaderboard");

        let current_timestamp: Option<u64> = cmd("GET")
            .arg(&timestamp_key)
            .query_async(&mut redis_conn)
            .await?;

        // Only update if:
        // 1. No previous timestamp exists (new tile)
        // 2. New timestamp is greater than the current one
        if let Some(current_ts) = current_timestamp {
            if click.timestamp_ns <= current_ts {
                info!(
               "Ignoring outdated update for tile {} (current: {}, received: {})",
               tile_id, current_ts, click.timestamp_ns
           );
                message.ack().await.map_err(|e| PollingConsumerError::Processing(e.to_string()))?;
                return Ok(());
            }
        }

        deadpool_redis::redis::pipe()
            .atomic()
            .cmd("SET").arg(&tile_key).arg(&click.country_id)
            .cmd("SET").arg(&timestamp_key).arg(click.timestamp_ns)
            .cmd("ZINCRBY").arg(&leaderboard_key).arg(1).arg(&click.country_id.to_string()) // Increment leaderboard score
            .query_async(&mut redis_conn)
            .await?;


        let processing_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;

        let span = Span::current();
        span.record("tile_id", tile_id);
        span.record("country", &click.country_id);
        span.record("message_timestamp", click.timestamp_ns);
        span.record("message_receive_time", receive_time);
        span.record("message_processing_time", processing_time);

        info!(
           "Tile {} is now owned by {} (timestamp: {})",
           tile_id, click.country_id, click.timestamp_ns
        );

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
