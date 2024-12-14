use opentelemetry::trace::{TracerProvider};
use opentelemetry_otlp::WithExportConfig;
use tracing_subscriber::{fmt, layer::SubscriberExt, EnvFilter, Registry};
use tracing_subscriber::fmt::Layer;
use tracing_subscriber::layer::Layered;

#[derive(Clone, Debug)]
pub struct TelemetryConfig {
    pub otlp_endpoint: String,
    pub service_name: String,
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            otlp_endpoint: "http://localhost:4317".to_string(),
            service_name: "clickplanet-server".to_string()
        }
    }
}

pub async fn init_telemetry(config: TelemetryConfig) -> Result<(), Box<dyn std::error::Error>> {
    let tracer_provider = opentelemetry_sdk::trace::TracerProvider::builder()
        .with_batch_exporter(
            opentelemetry_otlp::SpanExporter::builder()
                .with_endpoint(config.otlp_endpoint)
                .with_protocol(opentelemetry_otlp::Protocol::Grpc)
                .with_tonic()
                .build()?,
            opentelemetry_sdk::runtime::Tokio,
        )
        .build();

    let tracer = tracer_provider.tracer(config.service_name);

    // Create the OpenTelemetry layer
    let telemetry    = tracing_opentelemetry::layer().with_tracer(tracer);

    // Create a formatting layer for console output
    let fmt_layer: Layer<Layered<EnvFilter, Registry, Registry>> = fmt::layer()
        .with_target(false)
        .with_thread_ids(true)
        .with_thread_names(true)
        .with_file(true)
        .with_line_number(true);

    // Set up filter based on RUST_LOG env var, defaulting to info
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("debug"));

    // Combine both layers
    let subscriber = Registry::default()
        .with(env_filter)
        .with(fmt_layer)
        .with(telemetry);

    tracing::subscriber::set_global_default(subscriber)?;
    opentelemetry::global::shutdown_tracer_provider();

    Ok(())
}
