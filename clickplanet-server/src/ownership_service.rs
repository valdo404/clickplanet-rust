use async_nats::jetstream;
use async_nats::jetstream::consumer::pull::Stream;
use clickplanet_proto::clicks::{Click, Ownership, UpdateNotification};
use futures_util::stream::Map;
use futures_util::{future, StreamExt, TryStreamExt};
use prost::Message;
use std::error::Error;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::broadcast;
use tokio::sync::broadcast::Receiver;
use tokio::task::JoinHandle;
use tracing::{error, info, warn};

use crate::click_persistence::{ClickRepository, LeaderboardMaintainer, LeaderboardRepository};
use crate::nats_commons;
use crate::nats_commons::{get_stream, ConsumerConfig, PollingConsumerError};
use crate::redis_click_persistence::{RedisClickRepository, RedisPersistenceError};

const CONSUMER_NAME: &'static str = "tile-ownership-update";

#[derive(Error, Debug)]
pub enum ConsumerError {
    #[error("NATS consumer error: {0}")]
    NatsError(String),
    #[error("Failed to decode message: {0}")]
    DecodeError(#[from] prost::DecodeError),
    #[error("Processing error: {0}")]
    ProcessingError(String),
    #[error("Message acknowledgment failed: {0}")]
    AckError(String),
    #[error("Stream error: {0}")]
    StreamError(String),
    #[error("Polling error: {0}")]
    PollingConsumerError(#[from] PollingConsumerError),
}

#[derive(Clone)]
pub struct OwnershipUpdateService {
    click_repository: Arc<dyn ClickRepository>,
    leaderboard_maintainer: Arc<dyn LeaderboardMaintainer>,
    click_sender: Arc<broadcast::Sender<Click>>,
    update_tx: Arc<broadcast::Sender<UpdateNotification>>,
    jetstream: Arc<jetstream::Context>,
    consumer_config: ConsumerConfig,
}

impl OwnershipUpdateService {
    pub fn new(
        click_repository: Arc<dyn ClickRepository>,
        leaderboard_maintainer: Arc<dyn LeaderboardMaintainer>,
        click_sender: Arc<broadcast::Sender<Click>>,
        update_sender: Arc<broadcast::Sender<UpdateNotification>>,
        jetstream: Arc<jetstream::Context>,
        consumer_config: Option<ConsumerConfig>,
    ) -> Self {
        Self {
            click_repository,
            leaderboard_maintainer,
            click_sender,
            update_tx: update_sender,
            jetstream,
            consumer_config: consumer_config.unwrap_or_default(),
        }
    }

    pub async fn run(&self) -> Result<(), ConsumerError> {
        let click_rx = self.click_sender.subscribe();
        let nats_consumer: Stream = self.create_consumer().await?;
        let self_arc = Arc::new(self.clone());

        let nats_handle: JoinHandle<()> = self.clone().launch_nats_consumer(nats_consumer).await;
        let click_handle: JoinHandle<()> = self_arc.launch_direct_consumer(click_rx);

        tokio::select! {
            result = nats_handle => {
                if let Err(e) = result {
                    error!("NATS processing task failed: {:?}", e);
                } else {
                    error!("Unexpected NATS exit");
                }
            }
            result = click_handle => {
                if let Err(e) = result {
                    error!("Click processing task failed: {:?}", e);
                } else {
                    error!("Unexpected click handle exit");
                }
            }
        }

        Ok(())
    }

    fn launch_direct_consumer(self: Arc<Self>, click_rx: broadcast::Receiver<Click>) -> JoinHandle<()> {
        tokio::spawn({
            let this = self;
            let config = this.consumer_config.concurrent_processors;  // Get the config value before the move
            async move {
                futures::stream::unfold(click_rx, |mut rx| async {
                    match rx.recv().await {
                        Ok(click) => Some((click, rx)),
                        Err(_) => None,
                    }
                })
                    .map(move |click| {
                        let this = this.clone();
                        async move {
                            if let Err(e) = this.process_click(click).await {
                                error!("Error processing broadcast click: {:?}", e);
                            }
                        }
                    })
                    .buffer_unordered(config)  // Use the config value we captured earlier
                    .for_each(|_| future::ready(()))
                    .await;
            }
        })
    }

    async fn launch_nats_consumer(self, stream: Stream) -> JoinHandle<()> {
        let self_arc = Arc::new(self);

        tokio::spawn({
            let self_arc = self_arc.clone();
            let config = self_arc.consumer_config.concurrent_processors;
            let stream = stream;

            async move {
                if let Err(e) = process_nats_messages(
                    stream,
                    self_arc,
                    config
                ).await {
                    error!("NATS message processing failed: {:?}", e);
                }
            }
        })
    }

    async fn handle_nats_message(&self, message: jetstream::Message) -> Result<(), ConsumerError> {
        let click: Click = match clickplanet_proto::clicks::Click::decode(message.payload.clone()) {
            Ok(click) => click,
            Err(e) => {
                error!("Failed to decode message payload: {}", e);
                // Try to ack malformed messages to avoid redelivery
                if let Err(ack_err) = message.ack().await {
                    error!("Failed to ack malformed message: {}", ack_err);
                }
                return Ok(());
            }
        };

        match self.process_click(click).await {
            Ok(_) => {
                if let Err(e) = message.ack().await {
                    error!("Failed to acknowledge message after successful processing: {}", e);
                }
                Ok(())
            }
            Err(e) => {
                error!("Failed to process click: {}", e);
                if let Err(ack_err) = message.ack().await {
                    error!("Also failed to acknowledge failed message: {}", ack_err);
                }
                Ok(())
            }
        }
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
        let previous_ownership: Option<Ownership> = self.click_repository.save_click(click.tile_id as u32, &click).await?;

        // Only process ownership change if:
        // 1. There was a previous owner (Some) AND
        // 2. The previous country_id is different from the current one
        if let Some(last_ownership) = previous_ownership {
            if last_ownership.country_id != click.country_id {
                let notification = UpdateNotification {
                    tile_id: click.tile_id.try_into().unwrap(),
                    previous_country_id: last_ownership.country_id,
                    country_id: click.country_id,
                };

                self.leaderboard_maintainer.update_country_index(click.tile_id as u32,
                                                                 notification.country_id.as_str(),
                                                                 Some(notification.previous_country_id.as_str())
                                                                     .filter(|string| !string.is_empty())).await;

                let result = self.update_tx.send(notification);
                if let Err(e) = result {
                    tracing::debug!("No listener for ownership update: {:?}", e);
                }
            }
        }

        Ok(())
    }
}

async fn process_nats_messages(
    nats_stream: Stream,
    owner: Arc<OwnershipUpdateService>,
    config: usize,
) -> Result<(), ConsumerError> {
    nats_stream
        .map(|message_result| {
            message_result.map_err(|e| ConsumerError::ProcessingError(e.to_string()))
        })
        .try_for_each_concurrent(config, |message| {
            let owner = owner.clone();

            async move {
                if let Err(e) = owner.handle_nats_message(message).await {
                    error!("Error processing NATS message: {}", e);
                }
                Ok(())
            }
        })
        .await?;

    Ok(())
}