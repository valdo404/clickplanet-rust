use async_nats::{jetstream};
use clickplanet_proto::clicks::{Click, UpdateNotification};
use futures::{future, StreamExt};
use prost::Message;
use std::sync::Arc;
use tracing::{error, info};
use crate::click_persistence::{ClickRepository, LeaderboardRepository};
use crate::redis_click_persistence::{RedisClickRepository};
use crate::nats_commons;
use crate::nats_commons::{get_stream, ConsumerConfig, PollingConsumerError};


const CONSUMER_NAME: &'static str = "tile-state-processor";

pub struct ClickConsumer {
    jetstream: Arc<jetstream::Context>,
    consumer_config: ConsumerConfig,
    click_repository: Arc<dyn ClickRepository>,
}

impl ClickConsumer {
    pub async fn new(nats_url: &str, consumer_config: Option<ConsumerConfig>,
                     redis_click_repository: RedisClickRepository) -> Result<Self, PollingConsumerError> {
        let client = async_nats::connect(nats_url).await?;
        let jetstream = async_nats::jetstream::new(client);

        Ok(Self {
            jetstream: Arc::new(jetstream),
            consumer_config: consumer_config.unwrap_or_default(),
            click_repository: Arc::new(redis_click_repository)
        })
    }

    pub async fn create_consumer(
        &self,
    ) -> Result<jetstream::consumer::pull::Stream, PollingConsumerError> {
        let stream = get_stream(self.jetstream.clone()).await?;

        let config = jetstream::consumer::pull::Config {
            durable_name: Some(CONSUMER_NAME.to_string()),
            deliver_policy: jetstream::consumer::DeliverPolicy::All,
            ack_policy: jetstream::consumer::AckPolicy::Explicit,
            ack_wait: self.consumer_config.ack_wait,
            max_deliver: self.consumer_config.max_deliver,
            name: Some(CONSUMER_NAME.to_string()),
            ..Default::default()
        };

        let consumer = stream
            .create_consumer(config)
            .await
            .map_err(|e| PollingConsumerError::Processing(e.to_string()))?;

        let messages = consumer
            .messages()
            .await
            .map_err(|e| PollingConsumerError::Processing(e.to_string()))?;

        Ok(messages)
    }

    pub async fn run(&self) -> Result<(), PollingConsumerError> {
        let consumer = self.create_consumer().await?;
        info!("Starting stream processor");

        consumer
            .map(|message_result| {
                async move {
                    match message_result {
                        Ok(msg) => {
                            info!("Processing message on subject: {}", msg.subject);
                            if let Err(e) = self.handle_message(msg).await {
                                error!("Error processing message: {}", e);
                            }
                        }
                        Err(e) => error!("Error receiving message: {}", e),
                    }
                }
            })
            .buffer_unordered(self.consumer_config.concurrent_processors)
            .for_each(|_| future::ready(()))
            .await;

        Ok(())
    }

    async fn handle_message(&self, message: jetstream::Message) -> Result<(), PollingConsumerError> {
        let subject = message.subject.as_str();

        let tile_id: u32 = subject
            .strip_prefix(nats_commons::CLICK_SUBJECT_PREFIX)
            .and_then(|id| id.parse().ok())
            .ok_or_else(|| PollingConsumerError::Processing("Invalid subject format".to_string()))?;
        let click: Click = clickplanet_proto::clicks::Click::decode(message.payload.clone())?;

        self.click_repository.save_click(tile_id, &click).await?;

        message
            .ack()
            .await
            .map_err(|e| PollingConsumerError::Processing(e.to_string()))?;

        Ok(())
    }
}

