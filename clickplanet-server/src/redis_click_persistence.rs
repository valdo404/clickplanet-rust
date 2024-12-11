use deadpool_redis::{redis, Config as RedisConfig, CreatePoolError, PoolError, Runtime};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use async_nats::ConnectError;
use clickplanet_proto::clicks::{Click, ClickResponse};
use tracing::{info, instrument, Span};
use deadpool_redis::redis::AsyncCommands;
use thiserror::Error;
use crate::jetstream_click_streamer::PollingConsumerError;

const TILES_KEY: &str = "tiles";

pub struct RedisClickRepository {
    redis_pool: Arc<deadpool_redis::Pool>,
}


#[derive(Error, Debug)]
pub enum RedisPersistenceError {
    #[error("Redis error: {0}")]
    Redis(#[from] deadpool_redis::redis::RedisError),
    #[error("Redis pool error: {0}")]
    RedisPool(#[from] PoolError),
    #[error("Redis create pool error: {0}")]
    CreateRedisPool(#[from] CreatePoolError),
}

impl RedisClickRepository {
    pub async fn new(redis_url: &str) -> Result<Self, RedisPersistenceError> {
        let redis_cfg = RedisConfig::from_url(redis_url);
        let redis_pool = redis_cfg.create_pool(Some(Runtime::Tokio1))?;

        Ok(Self {
            redis_pool: Arc::new(redis_pool),
        })
    }

    pub async fn get_tile(
        &self,
        tile_id: i32,
    ) -> Result<Option<ClickResponse>, RedisPersistenceError> {
        let mut redis_conn = self.redis_pool.get().await?;

        let value: Option<String> = redis_conn.zscore(TILES_KEY, tile_id.to_string()).await?;

        match value {
            Some(val) => {
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

    #[instrument(
        name = "save_click",
        skip(self),
        fields(
           tile_id = tracing::field::Empty,
           country = tracing::field::Empty,
           message_timestamp = tracing::field::Empty,
           message_receive_time = tracing::field::Empty,
           message_processing_time = tracing::field::Empty,
        )
    )]
    pub async fn save_click(&self, tile_id: i32, click: &Click) -> Result<(), RedisPersistenceError> {
        let receive_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;

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

        Ok(())
    }
}