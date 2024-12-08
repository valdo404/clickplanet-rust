
mod client;

pub use client::ClickPlanetRestClient;
pub use client::TileCount;

pub mod prelude {
    pub use super::ClickPlanetRestClient;
    pub use super::TileCount;
}