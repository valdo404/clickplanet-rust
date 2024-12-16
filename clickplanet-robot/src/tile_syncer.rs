use std::sync::Arc;
use std::time::Duration;
use std::collections::HashMap;
use futures_util::StreamExt;
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use tokio::time::sleep;
use clickplanet_proto::clicks::{UpdateNotification};
use clickplanet_client::TileCount;

#[derive(Clone)]
pub struct TileSyncer {
    prod_client: Arc<clickplanet_client::ClickPlanetRestClient>,
    local_client: Arc<clickplanet_client::ClickPlanetRestClient>,
    tile_coordinates_map: Arc<dyn TileCount + Send + Sync>,
}

#[derive(Debug)]
struct OwnershipDiff {
    tile_id: u32,
    prod_country: String,
    local_country: Option<String>,
}

impl TileSyncer {
    pub fn new(
        prod_client: Arc<clickplanet_client::ClickPlanetRestClient>,
        local_client: Arc<clickplanet_client::ClickPlanetRestClient>,
        tile_coordinates_map: Arc<dyn TileCount + Send + Sync>,
    ) -> Self {
        Self {
            prod_client,
            local_client,
            tile_coordinates_map,
        }
    }

    async fn compute_ownership_diff(&self) -> Result<Vec<OwnershipDiff>, Box<dyn std::error::Error + Send + Sync>> {
        let prod_ownerships = self.prod_client.get_ownerships(&self.tile_coordinates_map).await?;
        let local_ownerships = self.local_client.get_ownerships(&self.tile_coordinates_map).await?;

        let local_map: HashMap<u32, String> = local_ownerships
            .ownerships
            .into_iter()
            .map(|o| (o.tile_id, o.country_id))
            .collect();

        let diffs: Vec<OwnershipDiff> = prod_ownerships
            .ownerships
            .into_iter()
            .filter_map(|prod_ownership| {
                let local_country = local_map.get(&prod_ownership.tile_id).cloned();

                if local_country.as_ref() != Some(&prod_ownership.country_id) {
                    Some(OwnershipDiff {
                        tile_id: prod_ownership.tile_id,
                        prod_country: prod_ownership.country_id,
                        local_country,
                    })
                } else {
                    None
                }
            })
            .collect();

        Ok(diffs)
    }

    async fn sync_tiles(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let diffs = self.compute_ownership_diff().await?;
        println!("Found {} tiles with different ownership", diffs.len());

        futures::stream::iter(
            diffs.into_par_iter()
                .map(|diff| async move {
                    if let Err(e) = self.local_client.click_tile(diff.tile_id, &diff.prod_country).await {
                        eprintln!("Failed to sync tile {}: {}", diff.tile_id, e);
                    } else {
                        println!(
                            "Successfully synced tile {} from {:?} to {}",
                            diff.tile_id, diff.local_country, diff.prod_country
                        );
                    }
                })
                .collect::<Vec<_>>()
        )
            .buffer_unordered(8)
            .collect::<Vec<_>>()
            .await;

        Ok(())
    }

    async fn handle_update(&self, update: UpdateNotification) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        println!(
            "Received update for tile {}: applying new ownership {}",
            update.tile_id, update.country_id
        );

        if let Err(e) = self.local_client.click_tile(update.tile_id.try_into().unwrap(), &update.country_id).await {
            eprintln!("Failed to sync update for tile {}: {}", update.tile_id, e);
        } else {
            println!("Successfully synced tile {} to {}", update.tile_id, update.country_id);
        }

        Ok(())
    }

    async fn monitor_updates(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let updates = self.prod_client.listen_for_updates().await?;

        updates
            .for_each_concurrent(8, |update| async {
                if let Err(e) = self.handle_update(update).await {
                    eprintln!("Error handling update: {}", e);
                }
            })
            .await;

        Ok(())
    }

    pub async fn run(self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        println!("Starting diff-based tile sync between production and local");

        // Do an initial full diff and sync
        if let Err(e) = self.sync_tiles().await {
            eprintln!("Error during initial sync: {}", e);
        }

        // Clone for the periodic task
        let periodic_syncer = self.clone();

        // Spawn periodic diff and sync task
        let periodic_handle = tokio::spawn(async move {
            loop {
                sleep(Duration::from_secs(300)).await; // Run every 5 minutes
                if let Err(e) = periodic_syncer.sync_tiles().await {
                    eprintln!("Error during periodic sync: {}", e);
                }
            }
        });

        // Monitor websocket updates in the main task
        let update_handle = tokio::spawn(async move {
            loop {
                if let Err(e) = self.monitor_updates().await {
                    eprintln!("Error in update monitoring: {}. Reconnecting...", e);
                    sleep(Duration::from_secs(1)).await;
                }
            }
        });

        // Wait for either task to finish (they shouldn't under normal circumstances)
        tokio::select! {
            result = periodic_handle => {
                println!("Periodic sync task ended: {:?}", result);
            }
            result = update_handle => {
                println!("Update monitor task ended: {:?}", result);
            }
        }

        Ok(())
    }
}