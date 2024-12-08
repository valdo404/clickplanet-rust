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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let client = client::ClickPlanetRestClient::new("clickplanet.lol");
    let coordinates: CoordinatesData = read_coordinates_from_file()?;
    let index_coordinates: TileCoordinatesMap = coordinates.into();
    let geolookup: GeoLookup = GeoLookup::from_default_location()?;
    let country_tile_map: CountryTilesMap = CountryTilesMap::build(&geolookup, &index_coordinates)?;

    let watchguard: CountryWatchguard = CountryWatchguard::new(
        Arc::new(client),
        Arc::new(country_tile_map),
        Arc::new(index_coordinates),
        "fr",
        "fr"
    );

    watchguard.run().await?;

    Ok(())
}
