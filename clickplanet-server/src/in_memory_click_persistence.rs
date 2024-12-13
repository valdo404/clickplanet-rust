use crate::click_persistence::{ClickRepository, ClickRepositoryError, LeaderboardError, LeaderboardMaintainer, LeaderboardRepository};
use async_trait::async_trait;
use clickplanet_proto::clicks::{Click, Ownership, OwnershipState};
use papaya::{HashMap as PapayaMap, HashMapRef, HashSet, LocalGuard, Operation};
use std::collections::HashMap;
use std::hash::RandomState;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct TileData {
    pub country_id: String,
    pub timestamp_ns: u64,
}

#[derive(Clone)]
pub struct PapayaClickRepository {
    tiles: Arc<PapayaMap<u32, TileData>>,
    country_tiles: Arc<PapayaMap<String, Arc<HashSet<u32>>>>,
}

impl PapayaClickRepository {
    pub fn new() -> Self {
        Self {
            tiles: Arc::new(PapayaMap::new()),
            country_tiles: Arc::new(PapayaMap::new()),
        }
    }

    pub async fn populate_with(repository: Arc<dyn ClickRepository>) -> Result<Self, ClickRepositoryError> {
        let papaya= Self::new();

        let ownership_state: OwnershipState = repository.get_ownerships().await?;

        for ownership in ownership_state.ownerships {
            let tile_id = ownership.tile_id;
            papaya.save_click(tile_id, &Click{
                tile_id: tile_id as i32,
                country_id: ownership.country_id.clone(),
                timestamp_ns: ownership.timestamp_ns,
                click_id: "".to_string(),
            }).await?;

            papaya.update_country_index(tile_id, &ownership.country_id, None).await;
        }

        Ok(papaya)
    }

    fn new_tiles(tile_id: u32) -> Arc<HashSet<u32>> {
        let cloned_set = HashSet::new().clone();

        {
            let pinned = cloned_set.pin();
            pinned.insert(tile_id);
        }

        Arc::new(cloned_set)
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

#[async_trait]
impl LeaderboardMaintainer for PapayaClickRepository {
    async fn update_country_index<'a>(&self, tile_id: u32, new_country: &'a str, old_country: Option<&'a str>) {
        let country_map: HashMapRef<String, Arc<HashSet<u32>>, RandomState, LocalGuard> = self.country_tiles.pin();

        old_country.iter().for_each(|country| {
            country_map.compute(country.to_string(), |optional| {
                match optional {
                    Some((_, existing)) => {
                        let pinned_set = existing.pin();
                        pinned_set.remove(&tile_id);

                        if pinned_set.is_empty() {
                            Operation::<Arc<papaya::HashSet<u32>>, ()>::Insert(existing.clone())
                        } else {
                            Operation::Remove
                        }
                    }
                    None => {
                        Operation::Abort(())
                    }
                }
            });
        });

        country_map.update_or_insert(new_country.to_string(), |existing| {
            let set = existing.pin();
            set.insert(tile_id);
            existing.clone()
        }, Self::new_tiles(tile_id));
    }
}

#[async_trait]
impl LeaderboardRepository for PapayaClickRepository {
    async fn get_score(&self, country_id: &str) -> Result<u32, LeaderboardError> {
        let country_map = self.country_tiles.pin();
        let score = country_map
            .get(country_id)
            .map(|tiles|
                tiles.len() as u32)
            .unwrap_or(0);

        Ok(score)
    }

    async fn leaderboard(&self) -> Result<HashMap<String, u32>, LeaderboardError> {
        let country_map = self.country_tiles.pin();

        let scores: HashMap<String, u32> = country_map
            .iter()
            .map(|(country, tiles)|
                (country.clone(), tiles.len() as u32))
            .filter(|(_, score)| *score > 0)
            .collect();

        Ok(scores)
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::click_persistence::LeaderboardOnClicks;
    use std::collections::HashMap;
    use std::time::{SystemTime, UNIX_EPOCH};
    use uuid::Uuid;

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
        assert_eq!(ownership.country_id, "COUNTRY4"); // Last country should win
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


#[cfg(test)]
mod maintainer_tests {
    use super::*;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_update_and_scores() {
        let repo = PapayaClickRepository::new();

        repo.update_country_index(1, "country1", None);

        assert_eq!(repo.get_score("country1").await.unwrap(), 1);
        assert_eq!(repo.get_score("country2").await.unwrap(), 0);

        // Change tile ownership
        repo.update_country_index(1, "country2", Some("country1"));
        assert_eq!(repo.get_score("country1").await.unwrap(), 0);
        assert_eq!(repo.get_score("country2").await.unwrap(), 1);

        repo.update_country_index(2, "country2", None);
        repo.update_country_index(3, "country2", None);
        assert_eq!(repo.get_score("country2").await.unwrap(), 3);

        // Verify leaderboard
        let leaderboard = repo.leaderboard().await.unwrap();

        assert_eq!(leaderboard.get("country1"), None); // country1 should be removed as it has no tiles
        assert_eq!(leaderboard.get("country2"), Some(&3));
    }

    #[tokio::test]
    async fn test_concurrent_updates() {
        let repo = Arc::new(PapayaClickRepository::new());
        let mut handles = vec![];

        for i in 0..10 {
            let repo = repo.clone();
            let handle = tokio::spawn(async move {
                repo.update_country_index(i, "country1", None);
            });
            handles.push(handle);
        }

        // Wait for all updates to complete
        for handle in handles {
            handle.await.unwrap();
        }

        assert_eq!(repo.get_score("country1").await.unwrap(), 10);

        let mut handles = vec![];
        for i in 0..10 {
            let repo = repo.clone();
            let handle = tokio::spawn(async move {
                repo.update_country_index(i, "country2", Some("country1"));
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.await.unwrap();
        }
        let leaderboard = repo.leaderboard().await.unwrap();
        println!("Leaderboard {:?}", leaderboard);

        assert_eq!(repo.get_score("country1").await.unwrap(), 0);
        assert_eq!(repo.get_score("country2").await.unwrap(), 10);

        assert_eq!(leaderboard.get("country1"), None);
        assert_eq!(leaderboard.get("country2"), Some(&10));
    }

    #[tokio::test]
    async fn test_empty_country_removal() {
        let repo = PapayaClickRepository::new();

        // Add and remove tile from country1
        repo.update_country_index(1, "country1", None);
        assert_eq!(repo.get_score("country1").await.unwrap(), 1);

        repo.update_country_index(1, "country2", Some("country1"));
        assert_eq!(repo.get_score("country1").await.unwrap(), 0);

        // Verify country1 is removed from leaderboard
        let leaderboard = repo.leaderboard().await.unwrap();
        assert!(!leaderboard.contains_key("country1"));
        assert_eq!(leaderboard.get("country2"), Some(&1));
    }
}