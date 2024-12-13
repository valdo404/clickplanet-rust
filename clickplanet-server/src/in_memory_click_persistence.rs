use std::collections::HashMap;
use std::sync::Arc;
use papaya::HashMap as PapayaMap;
use async_trait::async_trait;
use clickplanet_proto::clicks::{Click, Ownership, OwnershipState};
use crate::click_persistence::{ClickRepository, ClickRepositoryError, LeaderboardRepository};

#[derive(Debug, Clone)]
pub struct TileData {
    pub country_id: String,
    pub timestamp_ns: u64,
}

#[derive(Clone)]
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};
    use uuid::{uuid, Uuid};
    use clickplanet_proto::clicks::UpdateNotification;
    use crate::click_persistence::LeaderboardOnClicks;

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
        let repository = PapayaClickRepository::new();
        let click_repo = Arc::new(repository.clone());
        let mut leaderboard_handles = Vec::new();

        for i in 0..10 {
            let repo = click_repo.clone();  // Clone the Arc before moving into spawn
            let handle = tokio::spawn(async move {
                let click = Click {
                    tile_id: i,
                    country_id: format!("COUNTRY{}", i % 2),
                    timestamp_ns: (10 + i * 10) as u64,
                    click_id: Uuid::new_v4().to_string(),
                };

                println!("Processing click: {:?}", click);
                repo.save_click(click.tile_id as u32, &click).await
            });

            leaderboard_handles.push(handle);
        }

        // Wait for all clicks to be processed
        for handle in leaderboard_handles {
            handle.await.unwrap().unwrap();
        }

        // Create leaderboard computation after all clicks are processed
        let leader_board_computation = LeaderboardOnClicks(repository);

        // Check scores
        let score0 = leader_board_computation.get_score("COUNTRY0").await.unwrap();
        let score1 = leader_board_computation.get_score("COUNTRY1").await.unwrap();
        let score2 = leader_board_computation.get_score("COUNTRY2").await.unwrap();

        // Get and verify leaderboard
        let leaderboard = leader_board_computation.leaderboard().await.unwrap();

        let expected_map = {
            let mut map = HashMap::new();
            map.insert("COUNTRY0".to_string(), 5);
            map.insert("COUNTRY1".to_string(), 5);
            map
        };

        assert_eq!(leaderboard, expected_map);
        assert_eq!(score0 + score1 + score2, 10);
    }
}