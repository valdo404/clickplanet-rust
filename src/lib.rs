//! A client library for interacting with ClickPlanet
//!
//! This crate provides functionality to:
//! - Connect to ClickPlanet WebSocket stream
//! - Listen for tile updates
//! - Send click actions
//! - Query ownership state

mod client;
mod coordinates;

pub use client::Client;
pub use crate::client::clicks;  // Re-export the generated protobuf types

/// Re-exports commonly used types
pub mod prelude {
    pub use super::Client;
    pub use super::clicks::{
        UpdateNotification,
        ClickRequest,
        BatchRequest,
        OwnershipState,
        MapDensityResponse,
    };
}