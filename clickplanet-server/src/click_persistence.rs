use axum::async_trait;
use thiserror::Error;
use clickplanet_proto::clicks::{Click, Ownership, OwnershipState, UpdateNotification};

#[derive(Error, Debug)]
pub enum ClickRepositoryError {
    #[error("Storage error: {0}")]
    StorageError(String),
    #[error("Invalid data format: {0}")]
    InvalidDataError(String),
    #[error(transparent)]
    Other(#[from] Box<dyn std::error::Error + Send + Sync>),
}

#[async_trait]
pub trait ClickRepository: Send + Sync {
    async fn get_tile(&self, tile_id: u32) -> Result<Option<Ownership>, ClickRepositoryError>;

    async fn get_ownerships(&self) -> Result<OwnershipState, ClickRepositoryError>;

    async fn get_ownerships_by_batch(
        &self,
        start_tile_id: u32,
        end_tile_id: u32,
    ) -> Result<OwnershipState, ClickRepositoryError>;

    async fn save_click(&self, tile_id: u32, click: &Click) -> Result<Option<Ownership>, ClickRepositoryError>;
}

#[derive(Error, Debug)]
pub enum LeaderboardError {
    #[error("Storage error: {0}")]
    StorageError(String),
}

#[async_trait]
pub trait LeaderboardRepository: Send + Sync {
    async fn increment_score(&self, country_id: &str) -> Result<(), LeaderboardError>;
    async fn decrement_score(&self, country_id: &str) -> Result<(), LeaderboardError>;
    async fn get_score(&self, country_id: &str) -> Result<u32, LeaderboardError>;
    async fn process_ownership_change(
        &self,
        update_notification: &UpdateNotification,
    ) -> Result<(), LeaderboardError>;
    async fn leaderboard(&self) -> Result<std::collections::HashMap<String, u32>, LeaderboardError>;

}