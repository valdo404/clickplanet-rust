mod tile_syncer;
mod coordinates;
mod model;

use std::sync::Arc;
use clap::Parser;
use clickplanet_client::ClickPlanetRestClient;
use crate::tile_syncer::TileSyncer;
use crate::coordinates::{read_coordinates_from_file, CoordinatesData, TileCoordinatesMap};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Production server host
    #[arg(long, default_value = "clickplanet.lol")]
    prod_host: String,

    /// Production server port
    #[arg(long, default_value_t = 443)]
    prod_port: u16,

    #[arg(long, default_value = "coordinates.json")]
    coordinates_file: String,

    /// Local server host
    #[arg(long, default_value = "localhost")]
    local_host: String,

    /// Local server port
    #[arg(long, default_value_t = 3000)]
    local_port: u16,

    /// Disable TLS for prod server (secure by default)
    #[arg(long, default_value_t = false)]
    prod_unsecure: bool,

    /// Disable TLS for local server (unsecure by default)
    #[arg(long, default_value_t = true)]
    local_unsecure: bool,
}

#[tokio::main(flavor = "multi_thread", worker_threads = 10)]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let args = Args::parse();

    rustls::crypto::ring::default_provider().install_default()
        .expect("Failed to install crypto provider");

    let prod_client = ClickPlanetRestClient::new(
        &args.prod_host,
        args.prod_port,
        !args.prod_unsecure,
    );
    let local_client = ClickPlanetRestClient::new(
        &args.local_host,
        args.local_port,
        !args.local_unsecure,
    );

    println!("Initializing tile sync between {} and {}", args.prod_host, args.local_host);

    let coordinates: CoordinatesData = read_coordinates_from_file(&args.coordinates_file)?;
    let index_coordinates: TileCoordinatesMap = coordinates.into();

    let syncer = TileSyncer::new(
        Arc::new(prod_client),
        Arc::new(local_client),
        Arc::new(index_coordinates)
    );

    syncer.run().await?;

    Ok(())
}