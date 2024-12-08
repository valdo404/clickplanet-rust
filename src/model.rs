use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Position3D {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct UVCoordinate {
    pub u: f64,
    pub v: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TileVertex {
    pub index: usize,
    pub position: Position3D,
    pub uv: UVCoordinate,
}

impl TileVertex {
    pub fn new(index: usize, x: f64, y: f64, z: f64, u: f64, v: f64) -> Self {
        Self {
            index: index,
            position: Position3D { x, y, z },
            uv: UVCoordinate { u, v },
        }
    }
}