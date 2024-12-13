use crate::click_persistence::{ClickRepository, ClickRepositoryError};
use async_trait::async_trait;
use clickplanet_proto::clicks::UpdateNotification;
use clickplanet_proto::clicks::{Click, Ownership, OwnershipState};
use deadpool_redis::redis::AsyncCommands;
use deadpool_redis::{redis, Config as RedisConfig, CreatePoolError, PoolError, Runtime};
use log::error;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::in_memory_click_persistence::PapayaClickRepository;
use thiserror::Error;
use tracing::{debug, info, instrument, Span};

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



#[cfg(test)]
mod click_tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};
    use testcontainers::runners::AsyncRunner;
    use testcontainers::*;
    use testcontainers_modules::redis::{Redis, REDIS_PORT};

    async fn create_test_repo() -> (RedisClickRepository, ContainerAsync<Redis>) {
        let redis_instance = Redis::default()
            .start().await.unwrap();
        let host_ip = redis_instance.get_host().await.unwrap();
        let host_port: u16 = redis_instance.get_host_port_ipv4(REDIS_PORT).await.unwrap();
        println!("Host {} port {} & instance {}", host_ip, host_port, redis_instance.id());
        let redis_url = format!("redis://localhost:{}", host_port);
        let repo = RedisClickRepository::new(&redis_url).await.unwrap();
        println!("Created repo for {}", redis_url);

        (repo, redis_instance)
    }

    fn create_test_click(tile_id: u32, country_id: &str) -> Click {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;

        Click {
            tile_id: tile_id as i32,
            country_id: country_id.to_string(),
            timestamp_ns: now,
            click_id: format!("test_click_{}", tile_id),
        }
    }

    #[tokio::test]
    async fn test_save_and_get_tile() {
        let (repo, _container) = create_test_repo().await;

        // Test saving new ownership
        let click = create_test_click(1, "country1");
        let ownership = repo.save_click(1, &click).await.unwrap();

        assert!(ownership.is_none());

        // Test getting saved tile
        let fetched = repo.get_tile(1).await.unwrap();
        assert!(fetched.is_some());
        let fetched_ownership = fetched.unwrap();
        assert_eq!(fetched_ownership.tile_id, 1);
        assert_eq!(fetched_ownership.country_id, "country1");
    }

    #[tokio::test]
    async fn test_get_ownerships() {
        let (repo, _container) = create_test_repo().await;

        for i in 0..5 {
            let click = create_test_click(i, &format!("country{}", i % 2));
            repo.save_click(i, &click).await.unwrap();
        }

        // Get all ownerships
        let state = repo.get_ownerships().await.unwrap();
        assert_eq!(state.ownerships.len(), 5);

        // Verify ownerships
        let mut country0_count = 0;
        let mut country1_count = 0;

        for ownership in state.ownerships {
            match ownership.country_id.as_str() {
                "country0" => country0_count += 1,
                "country1" => country1_count += 1,
                _ => panic!("Unexpected country_id"),
            }
        }

        assert_eq!(country0_count, 3); // tiles 0, 2, 4
        assert_eq!(country1_count, 2); // tiles 1, 3
    }

    #[tokio::test]
    async fn test_get_ownerships_by_batch() {
        let (repo, _container) = create_test_repo().await;

        // Save tiles 0-9
        for i in 0..10 {
            let click = create_test_click(i, &format!("country{}", i % 3));
            repo.save_click(i, &click).await.unwrap();
        }

        // Test batch retrieval
        let batch = repo.get_ownerships_by_batch(2, 6).await.unwrap();
        assert_eq!(batch.ownerships.len(), 5); // tiles 2,3,4,5,6

        // Verify batch contents
        for ownership in batch.ownerships {
            assert!(ownership.tile_id >= 2 && ownership.tile_id <= 6);
            assert_eq!(ownership.country_id, format!("country{}", ownership.tile_id % 3));
        }
    }

    #[tokio::test]
    async fn test_ownership_updates() {
        let (repo, _container) = create_test_repo().await;

        let click1 = create_test_click(1, "country1");
        let ownership1 = repo.save_click(1, &click1).await.unwrap();
        assert_eq!(ownership1, None);

        let click2 = create_test_click(1, "country2");
        let ownership2 = repo.save_click(1, &click2).await.unwrap().unwrap();
        assert_eq!(ownership2.country_id, "country1");

        // Verify latest ownership
        let current = repo.get_tile(1).await.unwrap().unwrap();
        assert_eq!(current.country_id, "country2");
    }

    #[tokio::test]
    async fn test_concurrent_clicks() {
        let (repo, _container) = create_test_repo().await;
        let repo = Arc::new(repo);

        let handles: Vec<_> = (0..10)
            .map(|i| {
                let repo = repo.clone();
                tokio::spawn(async move {
                    let click = create_test_click(1, &format!("country{}", i));
                    repo.save_click(1, &click).await.unwrap();
                })
            })
            .collect();

        for handle in handles {
            handle.await.unwrap();
        }

        // Verify only one ownership exists for the tile
        let ownership = repo.get_tile(1).await.unwrap();
        assert!(ownership.is_some());
    }

    #[tokio::test]
    async fn test_error_handling() {
        let (repo, container) = create_test_repo().await;

        // Initial successful operation
        let click = create_test_click(1, "country1");
        repo.save_click(1, &click).await.unwrap();

        // Stop Redis
        container.stop().await.unwrap();

        // Operations should fail after stopping Redis
        let click = create_test_click(2, "country2");
        assert!(repo.save_click(2, &click).await.is_err());
        assert!(repo.get_tile(1).await.is_err());
        assert!(repo.get_ownerships().await.is_err());
        assert!(repo.get_ownerships_by_batch(1, 5).await.is_err());
    }
}