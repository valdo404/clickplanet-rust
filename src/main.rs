mod client;
mod coordinates;
mod model;
mod geolookup;
mod country_watchguard;

use std::error::Error;
use crate::coordinates::{read_coordinates_from_file, CoordinatesData, TileCoordinatesMap};
use crate::country_watchguard::CountryWatchguard;
use crate::geolookup::{CountryTilesMap, GeoLookup};
use futures::StreamExt;
use std::sync::Arc;

use clap::Parser;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Target country code (e.g., "fr", "de")
    #[arg(long, default_value = "fr")]
    target_country: String,

    /// Wanted country code (e.g., "fr", "de")
    #[arg(long, default_value = "fr")]
    wanted_country: String,

    /// Server hostname
    #[arg(long, default_value = "clickplanet.lol")]
    click_planet_host: String,

    /// Path to coordinates file
    #[arg(long, default_value = "coordinates.json")]
    coordinates_file: String,

    /// Path to geojson file
    #[arg(long, default_value = "countries.geojson")]
    geojson_file: String,
}


#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let args = Args::parse();

    let client = client::ClickPlanetRestClient::new(&args.click_planet_host);
    let coordinates: CoordinatesData = read_coordinates_from_file(&args.coordinates_file)?;
    let index_coordinates: TileCoordinatesMap = coordinates.into();
    let geolookup: GeoLookup = GeoLookup::from_file(&args.geojson_file)?;
    let country_tile_map: CountryTilesMap = CountryTilesMap::build(&geolookup, &index_coordinates)?;

    println!("Initializing watchguard for {} -> {}", args.target_country, args.wanted_country);

    let watchguard: CountryWatchguard = CountryWatchguard::new(
        Arc::new(client),
        Arc::new(country_tile_map),
        Arc::new(index_coordinates),
        &args.target_country.to_lowercase(),
        &args.wanted_country.to_lowercase()
    );

    watchguard.run().await?;

    Ok(())
}
