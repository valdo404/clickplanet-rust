use async_nats::jetstream::consumer::{Config, Consumer};
use async_nats::jetstream::stream::Stream;
use async_nats::jetstream::Context;
use async_nats::{jetstream, ConnectError};
use clickplanet_proto::clicks::{Click, ClickResponse};
use deadpool_redis::redis::{AsyncCommands, AsyncIter, RedisError};
use deadpool_redis::{redis, redis::cmd, Config as RedisConfig, Connection, PoolError, Runtime};
use futures::stream::{self};
use futures::{future, StreamExt};
use prost::Message;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use thiserror::Error;
use tracing::{error, info, instrument, Span};

use crate::constants;
use crate::constants::CLICK_STREAM_NAME;

const TILES_KEY: &str = "tiles";

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
        let leaderboard_key = "leaderboard";
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

    pub async fn get_tile(
        &self,
        tile_id: i32,
    ) -> Result<Option<ClickResponse>, PollingConsumerError> {
        let mut redis_conn = self.redis_pool.get().await?;

        // Get the value from sorted set
        let value: Option<String> = redis_conn.zscore(TILES_KEY, tile_id.to_string()).await?;

        match value {
            Some(val) => {
                // Parse "country:timestamp" format
                let parts: Vec<&str> = val.split(':').collect();
                if parts.len() == 2 {
                    if let Ok(timestamp_ns) = parts[1].parse::<u64>() {
                        Ok(Some(ClickResponse {
                            timestamp_ns,
                            click_id: parts[0].to_string(),
                        }))
                    } else {
                        Ok(None)
                    }
                } else {
                    Ok(None)
                }
            }
            None => Ok(None),
        }
    }

    pub async fn get_all_tiles(
        &self,
    ) -> Result<HashMap<i32, Option<ClickResponse>>, PollingConsumerError> {
        let mut redis_conn = self.redis_pool.get().await?;
        let mut tiles = HashMap::new();

        // Get all members with scores from the sorted set
        let values: Vec<(String, String)> = redis_conn
            .zrange_withscores(TILES_KEY, 0, -1)
            .await?;

        for (tile_id_str, value) in values {
            if let Ok(tile_id) = tile_id_str.parse::<i32>() {
                let parts: Vec<&str> = value.split(':').collect();
                if parts.len() == 2 {
                    if let Ok(timestamp_ns) = parts[1].parse::<u64>() {
                        tiles.insert(tile_id, Some(ClickResponse {
                            timestamp_ns,
                            click_id: parts[0].to_string(),
                        }));
                    }
                }
            }
        }

        Ok(tiles)
    }

    #[instrument(
        name = "process_message",
        skip(self, message),
        fields(
           tile_id = tracing::field::Empty,
           country = tracing::field::Empty,
           message_timestamp = tracing::field::Empty,
           message_receive_time = tracing::field::Empty,
           message_processing_time = tracing::field::Empty,
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

        let click: Click = clickplanet_proto::clicks::Click::decode(message.payload.clone())?;

        info!(
           "Received click for tile {} (country: {}, timestamp: {})",
           tile_id, click.country_id, click.timestamp_ns
        );

        let mut redis_conn = self.redis_pool.get().await?;

        let current_value: Option<String> = redis_conn
            .zrangebyscore::<String, i32, i32, Vec<String>>(TILES_KEY.to_string(), tile_id, tile_id)
            .await?
            .get(0)
            .cloned();

        info!(
           "Current value for tile {} ({:?})",
           tile_id, current_value
        );

        // Check if update is needed
        if let Some(current_val) = current_value {
            let parts: Vec<&str> = current_val.split(':').collect();

            if parts.len() == 2 {
                if let Ok(current_ts) = parts[1].parse::<u64>() {
                    if click.timestamp_ns <= current_ts {
                        info!(
                            "Ignoring outdated update for tile {} (current: {}, received: {})",
                            tile_id, current_ts, click.timestamp_ns
                        );
                        message.ack().await.map_err(|e| PollingConsumerError::Processing(e.to_string()))?;
                        return Ok(());
                    }
                }
            }
        } else {
            info!("No key for tile {}", tile_id);
        }

        let new_value: String = format!("{}:{}", click.country_id, click.timestamp_ns);

        info!(
           "New value for tile {} ({:?})",
           tile_id, new_value
        );

        redis::pipe()
            .atomic()
            .zadd(TILES_KEY, new_value.clone(), tile_id as f64)
            .zincr("leaderboard", click.country_id.clone(), 1.0)
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
