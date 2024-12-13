use deadpool_redis::{redis, Config as RedisConfig, CreatePoolError, PoolError, Runtime};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use async_nats::ConnectError;
use clickplanet_proto::clicks::{Click, ClickResponse, Ownership, OwnershipState};
use tracing::{info, instrument, Span};
use deadpool_redis::redis::AsyncCommands;
use thiserror::Error;

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
    #[error("Bad redis key: {0}")]
    BadKeyError(String),
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
    ) -> Result<Option<Ownership>, RedisPersistenceError> {
        let mut redis_conn = self.redis_pool.get().await?;

        let tile_contents: Vec<String> = redis_conn
            .zrangebyscore_limit::<String, i32, i32, Vec<String>>(TILES_KEY.to_string(), tile_id, tile_id, 0,1)
            .await?;

        if let Some(val) = tile_contents.first() {
            let parts: Vec<&str> = val.split(':').collect();
            if parts.len() == 2 {
                if let Ok(timestamp_ns) = parts[1].parse::<u64>() {
                    return Ok(Some(Ownership {
                        tile_id: tile_id as u32,
                        country_id: parts[0].to_string(),
                    }));
                }
            }
        }

        Ok(None)
    }

    pub async fn get_ownerships_by_batch(
        &self,
        start_tile_id: i32,
        end_tile_id: i32,
    ) -> Result<OwnershipState, Box<dyn std::error::Error + Send + Sync>> {
        let mut redis_conn = self.redis_pool.get().await?;

        let tile_contents: Vec<(String, String)> = redis_conn
            .zrangebyscore_withscores(
                TILES_KEY,
                start_tile_id as isize,
                end_tile_id as isize,
            )
            .await?;

        let mut ownerships: Vec<Ownership> = Vec::new();

        for (contents, tile_id) in tile_contents {
            let parts: Vec<&str> = contents.split(':').collect();
            if parts.len() == 2 {
                if let Ok(timestamp_ns) = parts[1].parse::<u64>() {
                    ownerships.push(Ownership {
                        tile_id: tile_id.parse::<u32>()?,
                        country_id: parts[0].to_string(),
                    });
                }
            }
        }

        let ownership_state = OwnershipState { ownerships };

        Ok(ownership_state)
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
            } else {
                return Err(RedisPersistenceError::BadKeyError(current_val))
            }

        } else {
            info!("No key for tile {}", tile_id);
        }

        let new_value: String = format!("{}:{}", click.country_id, click.timestamp_ns);

        info!(
           "New value for tile {} ({:?})",
           tile_id, new_value
        );

        let old_values: Vec<String> = redis_conn
            .zrangebyscore(TILES_KEY, tile_id as f64, tile_id as f64)
            .await?;

        let mut pipe = redis::pipe();
        let mut pipe = pipe
            .atomic()
            .zadd(TILES_KEY, new_value.clone(), tile_id as f64)
            .zincr("leaderboard", click.country_id.clone(), 1.0);

        for old_value in old_values {
            if old_value != new_value {
                pipe = pipe.zrem(TILES_KEY, old_value);
            }
        }

        pipe.query_async(&mut redis_conn).await?;

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