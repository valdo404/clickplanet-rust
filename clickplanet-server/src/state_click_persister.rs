use std::time::Duration;
use clap::Parser;
use tracing::info;
use crate::redis_click_persistence::{RedisClickRepository};

mod jetstream_click_streamer;
mod nats_commons;
mod telemetry;
mod redis_click_persistence;
mod click_persistence;
mod in_memory_click_persistence;

use crate::nats_commons::ConsumerConfig;
use crate::jetstream_click_streamer::{ClickConsumer};
use crate::telemetry::{init_telemetry, TelemetryConfig};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(long, env = "NATS_URL", default_value = "nats://localhost:4222")]
    nats_url: String,

    #[arg(long, env = "REDIS_URL", default_value = "redis://localhost:6379")]
    redis_url: String,

    #[arg(long, env = "OTEL_EXPORTER_OTLP_ENDPOINT", default_value = "http://localhost:4317")]
    otlp_endpoint: String,

    #[arg(long, env = "SERVICE_NAME", default_value = "click-persister")]
    service_name: String,

    #[arg(long, env = "CONCURRENT_PROCESSORS", default_value = "8")]
    concurrent_processors: u16,

    #[arg(long, env = "ACK_WAIT_SECS", default_value = "10")]
    ack_wait_secs: u64,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    let telemetry_config = TelemetryConfig {
        otlp_endpoint: args.otlp_endpoint,
        service_name: args.service_name,
    };

    init_telemetry(telemetry_config).await?;

    let click_persister = RedisClickRepository::new(&args.redis_url).await?;

    let consumer = ClickConsumer::new(
        &args.nats_url,
        Some(ConsumerConfig {
            concurrent_processors: args.concurrent_processors as usize,
            ack_wait: Duration::from_secs(args.ack_wait_secs),
            ..Default::default()
        }),
        click_persister
    )
        .await?;

    info!("Starting click consumer...");
    consumer.run().await?;

    Ok(())
}

