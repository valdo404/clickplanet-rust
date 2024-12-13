mod coordinates;
mod geolookup;
mod country_watchguard;
mod model;

use std::error::Error;
use crate::coordinates::{read_coordinates_from_file, CoordinatesData, TileCoordinatesMap};
use crate::country_watchguard::CountryWatchguard;
use crate::geolookup::{CountryTilesMap, GeoLookup};
use futures::StreamExt;
use std::sync::Arc;
use clap::Parser;
use rustls::{ClientConfig, RootCertStore};

use clickplanet_client::ClickPlanetRestClient;


#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(long, default_value = "fr")]
    target_country: String,

    #[arg(long, default_value = "fr")]
    wanted_country: String,

    #[arg(long, default_value = "clickplanet.lol")]
    host: String,

    #[arg(long, default_value = "443")]
    port: u16,

    #[arg(long, default_value = "false")]
    unsecure: bool,

    #[arg(long, default_value = "coordinates.json")]
    coordinates_file: String,

    /// Path to geojson file
    #[arg(long, default_value = "countries.geojson")]
    geojson_file: String,
}


#[tokio::main(flavor = "multi_thread", worker_threads = 10)]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let args = Args::parse();
    rustls::crypto::ring::default_provider().install_default()
        .expect("Failed to install crypto provider");

    let runtime_handle = tokio::runtime::Handle::current();
    let coordinates: CoordinatesData = read_coordinates_from_file(&args.coordinates_file)?;
    let index_coordinates: TileCoordinatesMap = coordinates.into();
    let geolookup: GeoLookup = GeoLookup::from_file(&args.geojson_file)?;

    let country_tile_map = CountryTilesMap::load_or_build(&geolookup, &index_coordinates)?;
    let client = ClickPlanetRestClient::new(&args.host, args.port, !args.unsecure);

    println!("Initializing watchguard for {} -> {}", args.target_country, args.wanted_country);

    let watchguard: CountryWatchguard = CountryWatchguard::new(
        Arc::new(client),
        Arc::new(country_tile_map),
        Arc::new(index_coordinates),
        &args.target_country.to_lowercase(),
        &args.wanted_country.to_lowercase()
    );

    watchguard.run(runtime_handle).await?;

    Ok(())
}
