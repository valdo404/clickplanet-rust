use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use std::thread;
use uuid::Uuid;
use crate::backends::backend::{Ownerships, OwnershipsGetter, TileClicker, Update, UpdatesListener};

pub struct FakeBackend {
    tile_bindings: Arc<Mutex<HashMap<u32, String>>>,
    update_listeners: Arc<Mutex<HashMap<String, Box<dyn Fn(Update) + Send + Sync>>>>,
    pending_updates: Arc<Mutex<Vec<Update>>>,
    batch_update_callbacks: Arc<Mutex<HashMap<String, Box<dyn Fn(Vec<Update>) + Send + Sync>>>>,
}

impl FakeBackend {
    pub fn new(batch_update_duration_ms: u64, countries: Vec<String>) -> Arc<Self> {
        let backend = Arc::new(Self {
            tile_bindings: Arc::new(Mutex::new(HashMap::new())),
            update_listeners: Arc::new(Mutex::new(HashMap::new())),
            pending_updates: Arc::new(Mutex::new(Vec::new())),
            batch_update_callbacks: Arc::new(Mutex::new(HashMap::new())),
        });

        // Initialize tiles with default country "fr"
        {
            let mut bindings = backend.tile_bindings.lock().unwrap();
            for i in 1..=257_000 {
                bindings.insert(i, "fr".to_string());
            }
        }

        // Periodic batch updates
        {
            let backend_ref = Arc::clone(&backend);
            thread::spawn(move || {
                loop {
                    let updates = {
                        let mut pending_updates = backend_ref.pending_updates.lock().unwrap();
                        if pending_updates.is_empty() {
                            thread::sleep(Duration::from_millis(batch_update_duration_ms));
                            continue;
                        }
                        std::mem::take(&mut *pending_updates)
                    };

                    let batch_callbacks = backend_ref.batch_update_callbacks.lock().unwrap();
                    for callback in batch_callbacks.values() {
                        callback(updates.clone());
                    }
                }
            });
        }

        for country_code in countries {
            let backend_ref = Arc::clone(&backend);
            thread::spawn(move || {
                let mut tile_id = rand::random::<u32>() % 100_00;
                let gap = rand::random::<u32>() % 100;

                loop {
                    tile_id = (tile_id + gap) % 257_000;
                    if let Err(e) = backend_ref.click_tile(tile_id, country_code.clone()) {
                        eprintln!("Failed to click tile: {:?}", e);
                    }
                    thread::sleep(Duration::from_millis(rand::random::<u64>() % 1000));
                }
            });
        }

        backend
    }

    pub fn click_tile(&self, tile_id: u32, country_id: String) -> Result<(), String> {
        let mut bindings = self.tile_bindings.lock().unwrap();
        let previous_country = bindings.insert(tile_id, country_id.clone());
        let update = Update {
            tile: tile_id,
            previous_country,
            new_country: country_id
        };

        let listeners = self.update_listeners.lock().unwrap();
        for listener in listeners.values() {
            listener(update.clone());
        }
        Ok(())
    }
}

impl TileClicker for FakeBackend {
    fn click_tile(&mut self, _tile_id: u32, _country_id: String) -> () {
        unimplemented!()
    }
}

impl UpdatesListener for FakeBackend {
    fn listen_for_updates(
        &self,
        callback: Box<dyn Fn(Update) + Send + Sync>,
    ) -> () {
        let id = Uuid::new_v4().to_string();
        {
            let mut listeners = self.update_listeners.lock().unwrap();
            listeners.insert(id.clone(), callback);
        }

        Box::new(move || {
            let mut listeners = self.update_listeners.lock().unwrap();
            listeners.remove(&id);
        })()
    }

    fn listen_for_updates_batch(
        &mut self,
        callback: Box<dyn Fn(Vec<Update>) + Send + Sync>,
    ) -> () {
        let id = Uuid::new_v4().to_string();
        {
            let mut batch_callbacks = self.batch_update_callbacks.lock().unwrap();
            batch_callbacks.insert(id.clone(), callback);
        }

        Box::new(move || {
            let mut batch_callbacks = self.batch_update_callbacks.lock().unwrap();
            batch_callbacks.remove(&id);
        })()
    }
}

impl OwnershipsGetter for FakeBackend {
    fn get_current_ownerships_by_batch(
        &self,
        batch_size: usize,
        max_index: usize,
        callback: Box<dyn Fn(Ownerships) + Send + Sync>,
    ) {
        let bindings = self.tile_bindings.lock().unwrap();
        let keys: Vec<_> = bindings.keys().cloned().collect();
        let mut start = 0;

        while start < max_index {
            let end = (start + batch_size).min(max_index);
            let slice: HashMap<_, _> = keys[start..end]
                .iter()
                .map(|&k| (k, bindings.get(&k).cloned().unwrap_or_default()))
                .collect();

            callback(Ownerships { bindings: slice });
            start = end;
        }
    }
}
