use std::time::Duration;
use tracing::info;
mod redis_click_persistence_service;
mod constants;
mod telemetry;

use crate::redis_click_persistence_service::{ConsumerConfig, RedisTileStateBuilder};
use crate::telemetry::{init_telemetry, TelemetryConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_telemetry(TelemetryConfig::default()).await?;

    let consumer = RedisTileStateBuilder::new(
        "nats://localhost:4222",
        "redis://localhost:6379",
        Some(ConsumerConfig {
            concurrent_processors: 8,
            ack_wait: Duration::from_secs(10),
            ..Default::default()
        })
    )
        .await?;

    info!("Starting click consumer...");
    consumer.run().await?;

    Ok(())
}