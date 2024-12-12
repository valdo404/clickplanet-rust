use std::sync::Arc;
use tokio::sync::broadcast;
use tokio::sync::broadcast::Receiver;
use clickplanet_proto::clicks::{Click, UpdateNotification, Ownership};
use crate::redis_click_persistence::{RedisClickRepository, RedisPersistenceError};


pub struct OwnershipUpdateService {
    click_persistence: Arc<RedisClickRepository>,
    click_sender: Arc<broadcast::Sender<Click>>,
    update_tx: Arc<broadcast::Sender<UpdateNotification>>,
}

impl OwnershipUpdateService {
    pub fn new(
        click_persistence: Arc<RedisClickRepository>,
        click_sender: Arc<broadcast::Sender<Click>>,
        update_sender: Arc<broadcast::Sender<UpdateNotification>>,
    ) -> Self {
        Self {
            click_persistence,
            click_sender,
            update_tx: update_sender,
        }
    }

    pub async fn run(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut click_rx: Receiver<Click> = self.click_sender.subscribe();

        while let Ok(click) = click_rx.recv().await {
            if let Err(e) = self.process_click(click).await {
                tracing::error!("Error processing click for ownership update: {:?}", e);
                // Continue processing other clicks even if one fails
                continue;
            }
        }

        Ok(())
    }

    async fn process_click(&self, click: Click) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let current_ownership: Option<Ownership> = self.click_persistence.get_tile(click.tile_id).await?;

        tracing::info!("Current ownership: {:?}", current_ownership);

        // Check if the country has changed
        match &current_ownership {
            Some(ownership) if ownership.country_id == click.country_id => {
                tracing::info!("Skipping update notification - country hasn't changed");
                return Ok(());
            }
            _ => {}
        }

        // Build update notification
        let notification = UpdateNotification {
            tile_id: click.tile_id,
            country_id: click.country_id,
            previous_country_id: match current_ownership {
                Some(ownership) => ownership.country_id,
                None => String::new(),
            },
        };

        // Broadcast the update
        let result = self.update_tx.send(notification);

        if let Err(e) = result {
            tracing::error!("No listener for ownership update: {:?}", e);
        }

        Ok(())
    }
}