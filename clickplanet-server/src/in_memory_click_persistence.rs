use std::collections::HashMap;
use std::sync::Arc;
use papaya::HashMap as PapayaMap;
use async_trait::async_trait;
use clickplanet_proto::clicks::{Click, Ownership, OwnershipState, UpdateNotification};
use crate::click_persistence::{ClickRepository, ClickRepositoryError, LeaderboardError, LeaderboardRepository};

#[derive(Debug, Clone)]
pub struct TileData {
    pub country_id: String,
    pub timestamp_ns: u64,
}

pub struct PapayaClickRepository {
    tiles: Arc<PapayaMap<u32, TileData>>,
}

impl PapayaClickRepository {
    pub fn new() -> Self {
        Self {
            tiles: Arc::new(PapayaMap::new()),
        }
    }

    pub async fn populate_with(repository: Arc<dyn ClickRepository>) -> Result<Self, ClickRepositoryError> {
        let papaya = Self {
            tiles: Arc::new(PapayaMap::new()),
        };

        let ownership_state: OwnershipState = repository.get_ownerships().await?;

        for ownership in ownership_state.ownerships {
            let tile_id = ownership.tile_id;
            papaya.save_click(tile_id, &Click{
                tile_id: tile_id as i32,
                country_id: ownership.country_id,
                timestamp_ns: ownership.timestamp_ns,
                click_id: "".to_string(),
            }).await?;
        }

        Ok(papaya)
    }
}

impl Default for PapayaClickRepository {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ClickRepository for PapayaClickRepository {
    async fn get_tile(&self, tile_id: u32) -> Result<Option<Ownership>, ClickRepositoryError> {
        Ok(self.tiles.pin().get(&tile_id).map(|data| Ownership {
            tile_id: tile_id as u32,
            country_id: data.country_id.clone(),
            timestamp_ns: data.timestamp_ns,
        }))
    }

    async fn get_ownerships(&self) -> Result<OwnershipState, ClickRepositoryError> {
        let mut ownerships = Vec::new();

        // Use Papaya's iterator to get all tiles
        self.tiles.pin().iter().for_each(|(tile_id, v)| {
            ownerships.push(Ownership {
                tile_id: *tile_id as u32,
                country_id: v.country_id.clone(),
                timestamp_ns: v.timestamp_ns,
            })
        });

        Ok(OwnershipState { ownerships })
    }

    async fn get_ownerships_by_batch(
        &self,
        start_tile_id: u32,
        end_tile_id: u32,
    ) -> Result<OwnershipState, ClickRepositoryError> {
        let mut ownerships = Vec::new();

        // Use Papaya's scan feature which is more efficient than individual gets
        self.tiles.pin().iter().for_each(|(k, v)| {
            if *k >= start_tile_id && *k <= end_tile_id {
                ownerships.push(Ownership {
                    tile_id: *k as u32,
                    country_id: v.country_id.clone(),
                    timestamp_ns: v.timestamp_ns,
                });
            }
        });

        Ok(OwnershipState { ownerships })
    }

    async fn save_click(&self, tile_id: u32, click: &Click) -> Result<Option<Ownership>, ClickRepositoryError> {
        let map_ref = self.tiles.pin();

        let tile_data = map_ref.get(&tile_id);

        let previous_ownership = tile_data.map(|data| Ownership {
            tile_id,
            country_id: data.country_id.clone(),
            timestamp_ns: data.timestamp_ns,
        });

        if let Some(current_data) = tile_data {
            if click.timestamp_ns <= current_data.timestamp_ns {
                return Ok(previous_ownership);
            }
        }

        map_ref.insert(tile_id, TileData {
            country_id: click.country_id.clone(),
            timestamp_ns: click.timestamp_ns,
        });

        Ok(previous_ownership)
    }
}


pub struct PapayaLeaderboardRepository {
    scores: Arc<PapayaMap<String, u32>>,
}

impl PapayaLeaderboardRepository {
    pub fn new() -> Self {
        Self {
            scores: Arc::new(PapayaMap::new()),
        }
    }
}

impl Default for PapayaLeaderboardRepository {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl LeaderboardRepository for PapayaLeaderboardRepository {
    async fn increment_score(&self, country_id: &str) -> Result<(), LeaderboardError> {
        let scores = self.scores.pin();
        scores.update_or_insert(country_id.to_string(), |current| current + 1, 0);
        Ok(())
    }

    async fn decrement_score(&self, country_id: &str) -> Result<(), LeaderboardError> {
        let scores = self.scores.pin();
        scores.update_or_insert(country_id.to_string(), |current| {
            if *current == 0 {
                0
            } else {
                current.saturating_sub(1)
            }
        }, 0);
        Ok(())
    }

    async fn get_score(&self, country_id: &str) -> Result<u32, LeaderboardError> {
        let scores = self.scores.pin();

        Ok(*scores.get(country_id).unwrap_or(&0))
    }

    async fn process_ownership_change(
        &self,
        update_notification: &UpdateNotification,
    ) -> Result<(), LeaderboardError> {
        if !update_notification.previous_country_id.is_empty() {
            self.decrement_score(&update_notification.previous_country_id).await?;
        }

        self.increment_score(&update_notification.country_id).await?;

        Ok(())
    }

    async fn leaderboard(&self) -> Result<std::collections::HashMap<String, u32>, LeaderboardError> {
        let scores = self.scores.pin();
        let mut result = std::collections::HashMap::new();

        scores.iter().for_each(|(country_id, score)| {
            result.insert(country_id.clone(), *score);
        });

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};
    use clickplanet_proto::clicks::UpdateNotification;

    #[tokio::test]
    async fn test_concurrent_updates() {
        let repo = Arc::new(PapayaClickRepository::new());
        let tile_id = 1;
        let base_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;

        // Simulate multiple concurrent updates
        let mut handles = Vec::new();
        for i in 0..100 {
            let repo = repo.clone();
            let handle = tokio::spawn(async move {
                let click = Click {
                    tile_id: i,
                    click_id: "".to_string(),
                    country_id: format!("COUNTRY{}", i % 5),
                    timestamp_ns: base_time + i as u64,
                };
                repo.save_click(tile_id, &click).await
            });
            handles.push(handle);
        }

        // Wait for all updates
        for handle in handles {
            handle.await.unwrap().unwrap();
        }

        // Verify final state
        let ownership = repo.get_tile(tile_id).await.unwrap().unwrap();
        assert_eq!(ownership.country_id, "COUNTRY0"); // Last country should win
    }

    #[tokio::test]
    async fn test_leaderboard_accuracy() {
        let leaderboard_repo = Arc::new(PapayaLeaderboardRepository::new());

        let mut leaderboard_handles = Vec::new();

        for i in 0..10 {
            let leaderboard_repo = leaderboard_repo.clone(); // Clone for each task
            let handle = tokio::spawn(async move {
                let update = UpdateNotification {
                    tile_id: i,
                    country_id: format!("COUNTRY{}", i % 2),
                    previous_country_id: format!("COUNTRY{}", i % 2 + 1),
                };
                println!(
                    "Processing update: {:?} -> {:?}",
                    update.previous_country_id, update.country_id
                );

                leaderboard_repo.process_ownership_change(&update).await
            });

            leaderboard_handles.push(handle);
        }

        for handle in leaderboard_handles {
            handle.await.unwrap().unwrap();
        }


        // Now we can use the original repo reference

        let score0 = leaderboard_repo.get_score("COUNTRY0").await.unwrap();
        let score1 = leaderboard_repo.get_score("COUNTRY1").await.unwrap();
        let score2 = leaderboard_repo.get_score("COUNTRY2").await.unwrap();

        let leaderboard: HashMap<String, u32> = leaderboard_repo.leaderboard().await.unwrap();

        let map = {
            let mut map = HashMap::new();
            map.insert(
                "COUNTRY0".to_string(), 4
            );
            map.insert(
                "COUNTRY1".to_string(), 0
            );
            map.insert(
                "COUNTRY2".to_string(), 0
            );
            map
        };

        assert_eq!(leaderboard, map);

        assert_eq!(score0 + score1 + score2, 5);
    }
}