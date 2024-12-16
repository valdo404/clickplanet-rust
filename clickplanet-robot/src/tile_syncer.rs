use std::sync::Arc;
use std::time::Duration;
use futures_util::StreamExt;
use rayon::iter::IntoParallelIterator;
use tokio::time::sleep;
use clickplanet_proto::clicks::{OwnershipState, UpdateNotification};
use rayon::iter::ParallelIterator;
use clickplanet_client::TileCount;

#[derive(Clone)]
pub struct TileSyncer {
    prod_client: Arc<clickplanet_client::ClickPlanetRestClient>,
    local_client: Arc<clickplanet_client::ClickPlanetRestClient>,
    tile_coordinates_map: Arc<dyn TileCount + Send + Sync>,
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
    async fn sync_tiles(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Get ownership state from production using the tile coordinates map
        let prod_ownerships: OwnershipState = self.prod_client.get_ownerships(&self.tile_coordinates_map).await?;
        println!("Fetched {} ownerships from production", prod_ownerships.ownerships.len());

        // Process all ownership updates in parallel
        futures::stream::iter(
            prod_ownerships.ownerships
                .into_par_iter()
                .map(|ownership| async move {
                    println!("Syncing tile {} with country {}", ownership.tile_id, ownership.country_id);

                    if let Err(e) = self.local_client.click_tile(ownership.tile_id, &ownership.country_id).await {
                        eprintln!("Failed to sync tile {}: {}", ownership.tile_id, e);
                    } else {
                        println!("Successfully synced tile {} to {}", ownership.tile_id, ownership.country_id);
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
            "Received update for tile {}: {} -> {}",
            update.tile_id, update.previous_country_id, update.country_id
        );

        if let Err(e) = self.local_client.click_tile(update.tile_id.try_into().unwrap(), &update.country_id).await {
            eprintln!("Failed to sync update for tile {}: {}", update.tile_id, e);
        } else {
            println!("Successfully synced update for tile {} to {}", update.tile_id, update.country_id);
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
        println!("Starting tile sync between production and local");

        // Do an initial full sync
        if let Err(e) = self.sync_tiles().await {
            eprintln!("Error during initial sync: {}", e);
        }

        // Clone for the periodic task
        let periodic_syncer = self.clone();

        // Spawn periodic full sync task
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