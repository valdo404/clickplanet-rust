use crate::clicks::{ClickRequest, BatchRequest, OwnershipState, UpdateNotification};

pub struct Client {
    base_url: String,
}

impl Client {
    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.to_string(),
        }
    }

    pub async fn click_tile(&self, tile_id: i32, country_id: &str) -> Result<(), Box<dyn std::error::Error>> {
        let request = ClickRequest {
            tile_id,
            country_id: country_id.to_string(),
        };
        // Implement HTTP POST request here
        Ok(())
    }

    pub async fn get_ownerships(&self) -> Result<OwnershipState, Box<dyn std::error::Error>> {
        // Implement HTTP GET request here
        unimplemented!()
    }
}
