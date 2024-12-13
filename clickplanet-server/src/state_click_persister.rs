use std::time::Duration;
use tracing::info;
use crate::redis_click_persistence::RedisClickRepository;

mod jetstream_click_streamer;
mod nats_commons;
mod telemetry;
mod redis_click_persistence;
mod click_persistence;

use crate::nats_commons::ConsumerConfig;
use crate::jetstream_click_streamer::{ClickConsumer};
use crate::telemetry::{init_telemetry, TelemetryConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_telemetry(TelemetryConfig::default()).await?;
    let persister = RedisClickRepository::new("redis://localhost:6379").await?;

    let consumer = ClickConsumer::new(
        "nats://localhost:4222",
        Some(ConsumerConfig {
            concurrent_processors: 8,
            ack_wait: Duration::from_secs(10),
            ..Default::default()
        }),
        persister
    )
        .await?;

    info!("Starting click consumer...");
    consumer.run().await?;

    Ok(())
}

