use std::sync::Arc;
use async_nats::jetstream;
use futures_util::StreamExt;
use prost::Message;
use tokio::sync::broadcast;
use tokio::sync::broadcast::Receiver;
use tracing::error;
use clickplanet_proto::clicks::{Click, UpdateNotification, Ownership};
use crate::nats_commons;
use crate::nats_commons::{get_stream, ConsumerConfig, PollingConsumerError};
use crate::redis_click_persistence::{RedisClickRepository, RedisPersistenceError};

const CONSUMER_NAME: &'static str = "tile-ownership-update";

pub struct OwnershipUpdateService {
    click_persistence: Arc<RedisClickRepository>,
    click_sender: Arc<broadcast::Sender<Click>>,
    update_tx: Arc<broadcast::Sender<UpdateNotification>>,
    jetstream: Arc<jetstream::Context>,
    consumer_config: ConsumerConfig,
}

impl OwnershipUpdateService {
    pub fn new(
        click_persistence: Arc<RedisClickRepository>,
        click_sender: Arc<broadcast::Sender<Click>>,
        update_sender: Arc<broadcast::Sender<UpdateNotification>>,
        jetstream: Arc<jetstream::Context>,
        consumer_config: Option<ConsumerConfig>,
    ) -> Self {
        Self {
            click_persistence,
            click_sender,
            update_tx: update_sender,
            jetstream,
            consumer_config: consumer_config.unwrap_or_default(),
        }
    }

    pub async fn run(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut click_rx = self.click_sender.subscribe();

        let nats_consumer = self.create_consumer().await?;
        let mut nats_stream = nats_consumer
            .map(|message_result| {
                let this = self.clone();
                async move {
                    match message_result {
                        Ok(msg) => {
                            if let Err(e) = this.handle_nats_message(msg).await {
                                error!("Error processing NATS message: {}", e);
                            }
                        }
                        Err(e) => error!("Error receiving NATS message: {}", e),
                    }
                }
            })
            .buffer_unordered(self.consumer_config.concurrent_processors);

        loop {
            tokio::select! {
                result = click_rx.recv() => {
                    match result {
                        Ok(click) => {
                            if let Err(e) = self.process_click(click).await {
                                error!("Error processing broadcast click: {:?}", e);
                            }
                        }
                        Err(e) => {
                            error!("Error receiving broadcast message: {:?}", e);
                            break;
                        }
                    }
                }
                Some(_) = nats_stream.next() => {
                    continue;
                }
                else => break,
            }
        }

        Ok(())
    }

    async fn handle_nats_message(&self, message: jetstream::Message) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let subject = message.subject.as_str();

        let tile_id: i32 = subject
            .strip_prefix(nats_commons::CLICK_SUBJECT_PREFIX)
            .and_then(|id| id.parse().ok())
            .ok_or_else(|| PollingConsumerError::Processing("Invalid subject format".to_string()))?;

        let click: Click = clickplanet_proto::clicks::Click::decode(message.payload.clone())?;

        self.process_click(click).await?;

        message
            .ack()
            .await
            .map_err(|e| PollingConsumerError::Processing(e.to_string()))?;

        Ok(())
    }

    async fn create_consumer(&self) -> Result<jetstream::consumer::pull::Stream, PollingConsumerError> {
        let stream = get_stream(self.jetstream.clone()).await?;

        let config = jetstream::consumer::pull::Config {
            durable_name: Some(CONSUMER_NAME.to_string()),
            deliver_policy: jetstream::consumer::DeliverPolicy::All,
            ack_policy: jetstream::consumer::AckPolicy::Explicit,
            ack_wait: self.consumer_config.ack_wait,
            max_deliver: self.consumer_config.max_deliver,
            ..Default::default()
        };

        let consumer = stream
            .create_consumer(config)
            .await
            .map_err(|e| PollingConsumerError::Processing(e.to_string()))?;

        consumer
            .messages()
            .await
            .map_err(|e| PollingConsumerError::Processing(e.to_string()))
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