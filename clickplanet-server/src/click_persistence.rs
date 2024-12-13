use axum::async_trait;
use thiserror::Error;
use clickplanet_proto::clicks::{Click, Ownership, OwnershipState};

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
    async fn get_tile(&self, tile_id: i32) -> Result<Option<Ownership>, ClickRepositoryError>;

    async fn get_ownerships_by_batch(
        &self,
        start_tile_id: i32,
        end_tile_id: i32,
    ) -> Result<OwnershipState, ClickRepositoryError>;

    async fn save_click(&self, tile_id: i32, click: &Click) -> Result<(), ClickRepositoryError>;
}