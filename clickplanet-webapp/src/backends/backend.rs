use std::collections::HashMap;

/// Trait for handling tile clicks.
pub trait TileClicker {
    /// Handles a tile click.
    fn click_tile(&mut self, tile_id: u32, country_id: String);
}

/// Represents ownerships of tiles by countries.
#[derive(Debug, Clone)]
pub struct Ownerships {
    pub bindings: HashMap<u32, String>,
}

impl Ownerships {
    /// Creates a new, empty set of ownerships.
    pub fn new() -> Self {
        Ownerships {
            bindings: HashMap::new(),
        }
    }

    /// Adds or updates the ownership of a tile.
    pub fn set_binding(&mut self, tile_id: u32, country_id: String) {
        self.bindings.insert(tile_id, country_id);
    }

    /// Retrieves the current ownerships.
    pub fn get_bindings(&self) -> &HashMap<u32, String> {
        &self.bindings
    }
}

/// Trait for retrieving ownerships in batches.
pub trait OwnershipsGetter {
    fn get_current_ownerships_by_batch(
        &self,
        batch_size: usize,
        max_index: usize,
        callback: Box<dyn Fn(Ownerships) + Send + Sync>,
    );
}

/// Represents an update to tile ownership.
#[derive(Debug, Clone)]
pub struct Update {
    pub tile: u32,
    pub previous_country: Option<String>,
    pub new_country: String,
}

impl Update {
    /// Creates a new update instance.
    pub fn new(tile: u32, previous_country: Option<String>, new_country: String) -> Self {
        Update {
            tile,
            previous_country,
            new_country,
        }
    }
}

/// Trait for listening to updates.
pub trait UpdatesListener {
    /// Listens for individual updates and processes them using the provided callback.
    fn listen_for_updates(&self, callback: Box<dyn Fn(Update) + Send + Sync>) -> ();

    /// Listens for updates in batches and processes them using the provided callback.
    fn listen_for_updates_batch(
        &self,
        callback: Box<dyn Fn(Vec<Update>) + Send + Sync>,
    ) -> ();
}
