use crate::geolookup::CountryTilesMap;
use clickplanet_client::TileCount;
use futures_util::stream::BoxStream;
use futures_util::{StreamExt, TryStreamExt};
use rand::Rng;
use std::collections::HashSet;
use std::error::Error;
use std::future;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;
use rayon::iter::{IntoParallelIterator, IntoParallelRefIterator, ParallelIterator};
use rayon::ThreadPool;
use tokio::runtime::Runtime;
use tokio::time::{sleep, timeout};
use clickplanet_proto::clicks::{OwnershipState, UpdateNotification};

#[derive(Clone)]
pub struct CountryWatchguard {
    client: Arc<clickplanet_client::ClickPlanetRestClient>,
    tile_coordinates_map: Arc<dyn TileCount + Send + Sync>,
    country_tiles: HashSet<u32>,
    target_country: String,
    wanted_country: String,
}

impl CountryWatchguard {
    pub fn new(
        client: Arc<clickplanet_client::ClickPlanetRestClient>,
        country_tile_map: Arc<CountryTilesMap>,
        tile_coordinates_map: Arc<dyn TileCount + Send + Sync>,
        target_country: &str,
        wanted_country: &str,
    ) -> Self {
        let country_tiles = country_tile_map
            .get_tiles_for_country(target_country)
            .map(|tiles| tiles.iter().copied().collect())
            .unwrap_or_default();

        Self {
            client,
            country_tiles,
            tile_coordinates_map,
            target_country: target_country.to_string(),
            wanted_country: wanted_country.to_string(),
        }
    }


    async fn monitor_updates(self) -> Result<(), Box<dyn Error + Send + Sync>> {
        let updates: BoxStream<'_, clickplanet_proto::clicks::UpdateNotification> = self.client.listen_for_updates().await?;
        let country_tiles = self.country_tiles.clone();
        let wanted_country = self.wanted_country.clone();
        let this = self.clone();


        updates
            .filter({
                let wanted_country = wanted_country.clone();
                move |update| future::ready(
                    country_tiles.contains(&(update.tile_id as u32)) &&
                        update.country_id != wanted_country
                )
            })
            .map(move |update| {
                let this = this.clone();

                println!(
                    "Detected unauthorized change on tile {}: {} -> {}. Reclaiming...",
                    update.tile_id, update.previous_country_id, update.country_id
                );

                async move {
                    if let Err(e) = this.claim_tile(&(update.tile_id as u32)).await {
                        eprintln!("Error processing tile: {}", e);
                    }
                }
            })
            .buffer_unordered(4)
            .for_each(|_| future::ready(()))
            .await;

        Ok(())
    }

    async fn periodic_claim_check(self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        loop {
            println!("Performing periodic claim check...");
            self.claim_all_tiles().await?;
            println!();
            println!("Checker task completed");

            sleep(Duration::from_secs(120)).await;
        }
    }

    pub async fn run(self, handle: tokio::runtime::Handle) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        println!(
            "Starting watchguard for target country: {} / wanted country: {}",
            self.target_country,
            self.wanted_country
        );
        println!("Monitoring {} tiles", self.country_tiles.len());


        // Clone self for the second task
        let self_clone = Self {
            client: self.client.clone(),
            tile_coordinates_map: self.tile_coordinates_map.clone(),
            country_tiles: self.country_tiles.clone(),
            target_country: self.target_country.clone(),
            wanted_country: self.wanted_country.clone(),
        };

        let monitor = handle.spawn(async move {
            self.monitor_updates().await
        });

        let checker = handle.spawn(async move {
            self_clone.periodic_claim_check().await
        });

        tokio::select! {
            result = monitor => {
                println!("Monitor task completed: {:?}", result);
                return match result {
                    Ok(inner_result) => inner_result,
                    Err(join_error) => Err(Box::new(join_error) as Box<dyn Error + Send + Sync>)?
                }
            }
            result = checker => {
                return match result {
                    Ok(inner_result) => inner_result,
                    Err(join_error) => Err(Box::new(join_error) as Box<dyn Error + Send + Sync>)?
                }
            }
        }
    }

    async fn wait_with_jitter(&self) {
        let millis = rand::thread_rng().gen_range(500..=800);
        sleep(Duration::from_millis(millis)).await;
    }

    async fn claim_all_tiles(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let arc: Arc<dyn TileCount + Send + Sync> = self.tile_coordinates_map.clone();
        let ownerships: OwnershipState = self.client.get_ownerships(&(arc)).await?;

        let tiles_to_claim: HashSet<_> = self.find_tile_to_claim(ownerships);
        println!("Need to claim {} tiles", tiles_to_claim.len());

        futures::stream::iter(
            tiles_to_claim.into_par_iter()
                .map(|tile_id| async move {
                    println!("Claiming tile {}", tile_id);

                    if let Err(e) = self.claim_tile(&tile_id).await {
                        eprintln!("Failed to claim tile {}: {}", tile_id, e);
                    }
                })
                .collect::<Vec<_>>()
        )
            .buffer_unordered(4)
            .collect::<Vec<_>>()
            .await;

        Ok(())
    }

    async fn claim_tile(&self, tile_id: &u32) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {

        match timeout(
            Duration::from_secs(5),
            self.client.click_tile(*tile_id as i32, &self.wanted_country)
        ).await {
            Ok(result) => match result {
                Ok(_) => {
                    println!("Claimed tile {}", tile_id);
                }
                Err(e) => {
                    eprintln!("Failed to claim tile {}: {}", tile_id, e);
                    return Err(e);
                }
            },
            Err(_) => {
                eprintln!("Timeout while claiming tile {}", tile_id);
                return Err("Operation timed out after 5 seconds".into());
            }
        }

        self.wait_with_jitter().await;
        Ok(())
    }

    fn find_tile_to_claim(&self, ownerships: OwnershipState) -> HashSet<&u32> {
        self.country_tiles
            .iter()
            .filter(|&&tile_id| {
                ownerships.ownerships
                    .iter()
                    .find(|o| o.tile_id == tile_id)
                    .map_or(true, |o| o.country_id != self.wanted_country)
            })
            .collect()
    }
}