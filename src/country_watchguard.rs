use std::collections::HashSet;
use std::error::Error;
use std::sync::Arc;
use std::time::Duration;
use futures_util::stream::BoxStream;
use futures_util::StreamExt;
use rand::Rng;
use tokio::{runtime, task};
use tokio::runtime::Runtime;
use tokio::task::JoinHandle;
use tokio::time::sleep;
use crate::client;
use crate::coordinates::TileCoordinatesMap;
use crate::geolookup::CountryTilesMap;

#[derive(Clone)]
pub struct CountryWatchguard {
    client: Arc<client::ClickPlanetRestClient>,
    tile_coordinates_map: Arc<TileCoordinatesMap>,
    country_tiles: HashSet<u32>,
    target_country: String,
    wanted_country: String,
}

impl CountryWatchguard {
    pub fn new(
        client: Arc<client::ClickPlanetRestClient>,
        country_tile_map: Arc<CountryTilesMap>,
        tile_coordinates_map: Arc<TileCoordinatesMap>,
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

    async fn monitor_updates(self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut updates: BoxStream<'_, client::clicks::UpdateNotification> = self.client.listen_for_updates().await?;

        while let Some(update) = updates.next().await {
            if self.country_tiles.contains(&(update.tile_id as u32)) &&
                update.country_id != self.wanted_country {
                println!(
                    "Detected unauthorized change on tile {}: {} -> {}. Reclaiming...",
                    update.tile_id, update.previous_country_id, update.country_id
                );

                self.wait_with_jitter().await;

                match self.client.click_tile(update.tile_id, &self.wanted_country).await {
                    Ok(_) => println!("Successfully reclaimed tile {}", update.tile_id),
                    Err(e) => eprintln!("Failed to reclaim tile {}: {}", update.tile_id, e),
                }
            }
        }

        Ok(())
    }

    async fn periodic_claim_check(self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        loop {
            println!("Performing periodic claim check...");
            self.claim_all_tiles().await?;

            sleep(Duration::from_secs(300)).await;
        }
    }

    pub async fn run(self, handle: tokio::runtime::Handle) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        println!(
            "Starting watchguard for target country: {} / wanted_country: {}",
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
                println!("Checker task completed: {:?}", result);
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
        let ownerships = self.client.get_ownerships(&self.tile_coordinates_map).await?;

        let tiles_to_claim: HashSet<_> = self.country_tiles
            .iter()
            .filter(|&&tile_id| {
                ownerships.ownerships
                    .iter()
                    .find(|o| o.tile_id == tile_id)
                    .map_or(true, |o| o.country_id != self.target_country)
            })
            .collect();

        println!("Need to claim {} tiles", tiles_to_claim.len());

        for &tile_id in tiles_to_claim.iter() {
            match self.client.click_tile(*tile_id as i32, &self.target_country).await {
                Ok(_) => println!("Claimed tile {}", tile_id),
                Err(e) => eprintln!("Failed to claim tile {}: {}", tile_id, e),
            }

            self.wait_with_jitter().await;
        }

        Ok(())
    }
}