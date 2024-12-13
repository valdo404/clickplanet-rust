use deadpool_redis::{redis, Config as RedisConfig, CreatePoolError, PoolError, Runtime};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use clickplanet_proto::clicks::{Click, Ownership, OwnershipState};
use tracing::{debug, info, instrument, Span};
use deadpool_redis::redis::AsyncCommands;
use thiserror::Error;
use std::collections::HashMap;
use async_trait::async_trait;
use log::error;
use clickplanet_proto::clicks::UpdateNotification;
use crate::click_persistence::{LeaderboardRepository, LeaderboardError};
use crate::click_persistence::{ClickRepository, ClickRepositoryError};

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


#[derive(Error, Debug)]
pub enum RedisError {
    #[error("Redis error: {0}")]
    Redis(#[from] deadpool_redis::redis::RedisError),
    #[error("Redis pool error: {0}")]
    RedisPool(#[from] PoolError),
    #[error("Redis create pool error: {0}")]
    CreateRedisPool(#[from] CreatePoolError),
}

impl From<RedisError> for ClickRepositoryError {
    fn from(err: RedisError) -> Self {
        ClickRepositoryError::StorageError(err.to_string())
    }
}

impl RedisClickRepository {
    pub async fn new(redis_url: &str) -> Result<Self, RedisError> {
        let redis_cfg = RedisConfig::from_url(redis_url);
        let redis_pool = redis_cfg.create_pool(Some(Runtime::Tokio1))?;

        Ok(Self {
            redis_pool: Arc::new(redis_pool),
        })
    }
}

#[async_trait]
impl ClickRepository for RedisClickRepository {
    async fn get_tile(
        &self,
        tile_id: u32,
    ) -> Result<Option<Ownership>, ClickRepositoryError> {
        let mut redis_conn = self.redis_pool.get().await.map_err(RedisError::from)?;

        let tile_contents: Vec<String> = redis_conn
            .zrangebyscore_limit::<String, u32, u32, Vec<String>>(TILES_KEY.to_string(), tile_id, tile_id, 0, 1)
            .await
            .map_err(RedisError::from)?;

        if let Some(val) = tile_contents.first() {
            let parts: Vec<&str> = val.split(':').collect();
            if parts.len() == 2 {
                if let Ok(_) = parts[1].parse::<u64>() {
                    return Ok(Some(Ownership {
                        tile_id: tile_id as u32,
                        country_id: parts[0].to_string(),
                        timestamp_ns: parts[1].parse::<u64>().unwrap(),
                    }));
                }
            }
        }

        Ok(None)
    }

    async fn get_ownerships(&self) -> Result<OwnershipState, ClickRepositoryError> {
        let mut redis_conn = self.redis_pool.get().await.map_err(RedisError::from)?;

        // Get all tiles with their scores (-inf to +inf range)
        let tile_contents: Vec<(String, String)> = redis_conn
            .zrangebyscore_withscores(
                TILES_KEY,
                "-inf",  // Get from lowest possible score
                "+inf",  // to highest possible score
            )
            .await
            .map_err(RedisError::from)?;

        let mut ownerships = Vec::new();

        for (contents, tile_id) in tile_contents {
            let parts: Vec<&str> = contents.split(':').collect();
            if parts.len() == 2 {
                if let Ok(timestamp_ns) = parts[1].parse::<u64>() {
                    let tile_id = tile_id.parse::<u32>()
                        .map_err(|e| ClickRepositoryError::InvalidDataError(e.to_string()))?;

                    ownerships.push(Ownership {
                        tile_id,
                        country_id: parts[0].to_string(),
                        timestamp_ns,
                    });
                }
            }
        }

        Ok(OwnershipState { ownerships })
    }

    async fn get_ownerships_by_batch(
        &self,
        start_tile_id: u32,
        end_tile_id: u32,
    ) -> Result<OwnershipState, ClickRepositoryError> {
        let mut redis_conn = self.redis_pool.get().await.map_err(RedisError::from)?;

        let tile_contents: Vec<(String, String)> = redis_conn
            .zrangebyscore_withscores(
                TILES_KEY,
                start_tile_id as isize,
                end_tile_id as isize,
            )
            .await
            .map_err(RedisError::from)?;

        let mut ownerships = Vec::new();

        for (contents, tile_id) in tile_contents {
            let parts: Vec<&str> = contents.split(':').collect();
            if parts.len() == 2 {
                if let Ok(timestamp_ns) = parts[1].parse::<u64>() {
                    ownerships.push(Ownership {
                        tile_id: tile_id.parse::<u32>()
                            .map_err(|e| ClickRepositoryError::InvalidDataError(e.to_string()))?,
                        country_id: parts[0].to_string(),
                        timestamp_ns: timestamp_ns,
                    });
                }
            }
        }

        Ok(OwnershipState { ownerships })
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
    async fn save_click(&self, tile_id: u32, click: &Click) -> Result<Option<Ownership>, ClickRepositoryError> {
        let receive_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;

        debug!(
           "Received click for tile {} (country: {}, timestamp: {})",
           tile_id, click.country_id, click.timestamp_ns
        );

        let mut redis_conn = self.redis_pool.get().await.map_err(RedisError::from)?;

        let current_value: Option<String> = redis_conn
            .zrangebyscore::<String, u32, u32, Vec<String>>(TILES_KEY.to_string(), tile_id, tile_id)
            .await
            .map_err(RedisError::from)?
            .get(0)
            .cloned();

        debug!(
           "Current value for tile {} ({:?})",
           tile_id, current_value
        );

        // Create previous ownership object
        let previous_ownership: Option<Ownership> = if let Some(current_val) = &current_value {
            let parts: Vec<&str> = current_val.split(':').collect();
            if parts.len() == 2 {
                if let Ok(current_ts) = parts[1].parse::<u64>() {
                    let ownership = Some(Ownership {
                        tile_id,
                        country_id: parts[0].to_string(),
                        timestamp_ns: current_ts,
                    });

                    if click.timestamp_ns <= current_ts {
                        info!(
                            "Ignoring outdated update for tile {} (current: {}, received: {})",
                            tile_id, current_ts, click.timestamp_ns
                        );

                        return Ok(ownership);
                    }

                    ownership
                } else {
                    None
                }
            } else {
                return Err(ClickRepositoryError::InvalidDataError(current_val.clone()));
            }
        } else {
            debug!("No key for tile {}", tile_id);
            None
        };

        let new_value = format!("{}:{}", click.country_id, click.timestamp_ns);

        debug!(
           "New value for tile {} ({:?})",
           tile_id, new_value
        );

        let old_values: Vec<String> = redis_conn
            .zrangebyscore(TILES_KEY, tile_id as f64, tile_id as f64)
            .await
            .map_err(RedisError::from)?;

        let mut pipe = redis::pipe();
        let mut pipe = pipe
            .atomic()
            .zadd(TILES_KEY, new_value.clone(), tile_id as f64);

        for old_value in old_values {
            if old_value != new_value {
                pipe = pipe.zrem(TILES_KEY, old_value);
            }
        }

        pipe.query_async(&mut redis_conn)
            .await
            .map_err(RedisError::from)?;

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

        Ok(previous_ownership)
    }
}


const LEADERBOARD_KEY: &str = "leaderboard";

#[derive(Error, Debug)]
pub enum RedisLeaderboardError {
    #[error("Redis error: {0}")]
    Redis(#[from] deadpool_redis::redis::RedisError),
    #[error("Redis pool error: {0}")]
    RedisPool(#[from] PoolError),
    #[error("Redis create pool error: {0}")]
    CreateRedisPool(#[from] CreatePoolError),
}

impl From<RedisLeaderboardError> for LeaderboardError {
    fn from(err: RedisLeaderboardError) -> Self {
        LeaderboardError::StorageError(err.to_string())
    }
}

pub struct RedisLeaderboardRepository {
    redis_pool: Arc<deadpool_redis::Pool>,
}

impl RedisLeaderboardRepository {
    pub async fn new(redis_url: &str) -> Result<Self, RedisLeaderboardError> {
        let redis_cfg = RedisConfig::from_url(redis_url);
        let redis_pool = redis_cfg.create_pool(Some(Runtime::Tokio1))?;

        Ok(Self {
            redis_pool: Arc::new(redis_pool),
        })
    }
}

#[async_trait]
impl LeaderboardRepository for RedisLeaderboardRepository {
    async fn increment_score(&self, country_id: &str) -> Result<(), LeaderboardError> {
        let mut redis_conn = self.redis_pool.get().await.map_err(RedisLeaderboardError::from)?;

        redis_conn
            .zincr(LEADERBOARD_KEY, country_id, 1)
            .await
            .map_err(RedisLeaderboardError::from)?;

        Ok(())
    }

    async fn decrement_score(&self, country_id: &str) -> Result<(), LeaderboardError> {
        let mut redis_conn = self.redis_pool.get().await.map_err(RedisLeaderboardError::from)?;

        // Get current score
        let current_score: Option<u32> = redis_conn
            .zscore(LEADERBOARD_KEY, country_id)
            .await
            .map_err(RedisLeaderboardError::from)?;

        // Only decrement if score exists and is greater than 0
        if let Some(score) = current_score {
            if score > 0 {
                redis_conn
                    .zincr(LEADERBOARD_KEY, country_id, -1)
                    .await
                    .map_err(RedisLeaderboardError::from)?;
            }
        }

        Ok(())
    }

    async fn get_score(&self, country_id: &str) -> Result<u32, LeaderboardError> {
        let mut redis_conn = self.redis_pool.get().await.map_err(RedisLeaderboardError::from)?;

        let score: Option<u32> = redis_conn
            .zscore(LEADERBOARD_KEY, country_id)
            .await
            .map_err(RedisLeaderboardError::from)?;

        Ok(score.unwrap_or(0))
    }

    async fn process_ownership_change(
        &self,
        update: &UpdateNotification,
    ) -> Result<(), LeaderboardError> {
        let mut redis_conn = self.redis_pool.get().await.map_err(RedisLeaderboardError::from)?;

        let mut pipe = redis::pipe();

        // Decrement previous owner's score if it exists
        if !update.previous_country_id.is_empty() {
            let current_score: Option<u32> = redis_conn
                .zscore(LEADERBOARD_KEY, &update.previous_country_id)
                .await
                .map_err(RedisLeaderboardError::from)?;

            error!("Current score for {:?}, {:?}", current_score, update);

            if let Some(score) = current_score {
                if score > 0 {
                    pipe.zincr(LEADERBOARD_KEY, &update.previous_country_id, -1);
                }
            }
        }

        pipe.zincr(LEADERBOARD_KEY, &update.country_id, 1);
        pipe.query_async(&mut redis_conn)
            .await
            .map_err(RedisLeaderboardError::from)?;

        Ok(())
    }

    async fn leaderboard(&self) -> Result<HashMap<String, u32>, LeaderboardError> {
        let mut redis_conn = self.redis_pool.get().await.map_err(RedisLeaderboardError::from)?;

        let scores: Vec<(String, f64)> = redis_conn
            .zrange_withscores(LEADERBOARD_KEY, 0, -1)
            .await
            .map_err(RedisLeaderboardError::from)?;

        let result = scores
            .into_iter()
            .map(|(country_id, score)| (country_id, score as u32))
            .collect();

        Ok(result)
    }
}