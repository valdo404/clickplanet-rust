use std::cell::Cell;
use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use crate::model::TileVertex;


#[derive(Debug, Serialize, Deserialize)]
pub struct CoordinatesData {
    // Vector of 3D positions (x,y,z triplets)
    pub positions: Vec<f64>,
    // Vector of UV coordinates (u,v pairs)
    pub uvs: Vec<f64>,

    length_cache: Cell<Option<usize>>,
}

impl CoordinatesData {
    pub fn get_position_triplets(&self) -> impl Iterator<Item = &[f64]> {
        self.positions.chunks(3)
    }

    pub fn get_uv_pairs(&self) -> impl Iterator<Item = &[f64]> {
        self.uvs.chunks(2)
    }

    pub fn length(&self) -> usize {
        if let Some(len) = self.length_cache.get() {
            return len;
        }

        let len = self.positions.len() / 3;
        self.length_cache.set(Some(len));
        len
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.positions.len() % 3 != 0 {
            return Err("Positions length is not a multiple of 3".to_string());
        }

        if self.uvs.len() % 2 != 0 {
            return Err("UVs length is not a multiple of 2".to_string());
        }

        let position_vertices = self.positions.len() / 3;
        let uv_vertices = self.uvs.len() / 2;
        if position_vertices != self.length() || uv_vertices != self.length() {
            return Err(format!(
                "Inconsistent lengths: positions={}, uvs={}, length={}",
                position_vertices, uv_vertices, self.length()
            ));
        }

        Ok(())
    }
}


#[derive(Debug, Serialize, Deserialize)]
pub struct TileCoordinatesMap {
    pub tiles: HashMap<u32, TileVertex>,
}

impl TileCoordinatesMap {
    pub fn get_tile(&self, tile_id: u32) -> Option<&TileVertex> {
        self.tiles.get(&tile_id)
    }

    pub fn len(&self) -> usize {
        self.tiles.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tiles.is_empty()
    }
}

impl From<CoordinatesData> for TileCoordinatesMap {
    fn from(data: CoordinatesData) -> Self {
        let mut tiles = HashMap::new();
        let positions = data.positions.chunks(3);
        let uvs = data.uvs.chunks(2);

        // Zip together positions and uvs, enumerate to create tile IDs
        for (tile_id, (pos, uv)) in positions.zip(uvs).enumerate() {
            let vertex = TileVertex::new(
                tile_id,
                pos[0], pos[1], pos[2],  // xyz position
                uv[0], uv[1],           // uv coordinates
            );
            tiles.insert(tile_id as u32, vertex);
        }

        TileCoordinatesMap { tiles }
    }
}

pub fn read_coordinates_from_file() -> Result<CoordinatesData, Box<dyn std::error::Error + Send + Sync>> {
    let json_str = std::fs::read_to_string("coordinates.json")?;

    load_coordinates(&json_str)
}

pub fn load_coordinates(json_str: &str) -> Result<CoordinatesData, Box<dyn std::error::Error + Send + Sync>> {
    let coords: CoordinatesData = serde_json::from_str(json_str)?;
    coords.validate()?;
    Ok(coords)
}

#[derive(Debug, Clone)]
pub struct Vertex {
    pub position: [f64; 3],
    pub uv: [f64; 2],
}

impl CoordinatesData {
    pub fn to_vertices(&self) -> Vec<Vertex> {
        let positions = self.get_position_triplets();
        let uvs = self.get_uv_pairs();

        positions.zip(uvs)
            .map(|(pos, uv)| Vertex {
                position: [pos[0], pos[1], pos[2]],
                uv: [uv[0], uv[1]],
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_coordinates_deserialization() {
        let json = r#"{
            "positions": [-0.5943395495414734, 0.8018954992294312, 0.06102520972490311],
            "uvs": [0.5162845253944397, 0.796174943447113],
            "length": 1
        }"#;

        let coords = load_coordinates(json).unwrap();
        assert_eq!(coords.length(), 1);
        assert_eq!(coords.positions.len(), 3);
        assert_eq!(coords.uvs.len(), 2);

        let map: TileCoordinatesMap = coords.into();

        assert_eq!(map.len(), 1);
        assert_eq!(map.tiles.len(), 1);
    }
}