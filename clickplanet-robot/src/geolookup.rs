use crate::coordinates::TileCoordinatesMap;
use geojson::{GeoJson, Value};
use rstar::{PointDistance, RTree, RTreeObject, AABB};
use std::collections::HashMap;
use std::error::Error;
use rayon::prelude::*;
use crate::model::TileVertex;

#[derive(Debug, Clone, PartialEq)]
pub struct CountryPolygon {
    pub country_code: String,
    pub name: String,
    pub coordinates: Vec<Vec<(f64, f64)>>, // List of polygons (for countries with multiple parts)
}


#[derive(Debug, Default)]
pub struct CountryTilesMap {
    country_tiles: HashMap<String, Vec<u32>>,
    unassigned_tiles: Vec<u32>,
    tile_to_country: HashMap<u32, String>,
}

impl CountryTilesMap {
    fn new() -> Self {
        Self::default()
    }

    pub fn build(
        geo_lookup: &GeoLookup,
        tile_coords: &TileCoordinatesMap,
    ) -> Result<Self, Box<dyn Error + Send + Sync>> {
        let chunk_size = 1000;

        let mut map = Self::new();

        // Get and sort tile IDs
        let mut tile_ids: Vec<u32> = tile_coords.tiles.keys().copied().collect();
        tile_ids.sort_unstable();

        // Process tiles in parallel chunks
        let chunk_results: Vec<_> = tile_ids
            .par_chunks(chunk_size)
            .map(|chunk| {
                let mut local_country_tiles: HashMap<String, Vec<u32>> = HashMap::new();
                let mut local_unassigned: Vec<u32> = Vec::new();
                let mut local_tile_to_country: HashMap<u32, String> = HashMap::new();

                for &tile_id in chunk {
                    if let Some(tile_vertex) = tile_coords.tiles.get(&tile_id) {
                        if let Some(country_code) = geo_lookup.find_country(tile_vertex) {
                            let lower_country_code = country_code.to_lowercase();

                            local_country_tiles
                                .entry(lower_country_code.clone())
                                .or_default()
                                .push(tile_id);
                            local_tile_to_country.insert(tile_id, lower_country_code.clone());
                        } else {
                            local_unassigned.push(tile_id);
                        }
                    }
                }

                (local_country_tiles, local_unassigned, local_tile_to_country)
            })
            .collect();

        for (country_tiles, unassigned, tile_to_country) in chunk_results {
            for (country_code, tiles) in country_tiles {
                map.country_tiles
                    .entry(country_code)
                    .or_default()
                    .extend(tiles);
            }
            map.unassigned_tiles.extend(unassigned);
            map.tile_to_country.extend(tile_to_country);
        }

        // Sort the results
        for tiles in map.country_tiles.values_mut() {
            tiles.sort_unstable();
        }
        map.unassigned_tiles.sort_unstable();

        Ok(map)
    }

    pub fn is_unassigned(&self, tile_id: u32) -> bool {
        !self.tile_to_country.contains_key(&tile_id)
    }

    pub fn get_country_for_tile(&self, tile_id: u32) -> Option<&str> {
        self.tile_to_country.get(&tile_id).map(|s| s.as_str())
    }

    pub fn get_tiles_for_country(&self, country_code: &str) -> Option<&[u32]> {
        self.country_tiles.get(country_code).map(|v| v.as_slice())
    }

    pub fn get_unassigned_tiles(&self) -> &[u32] {
        &self.unassigned_tiles
    }

    pub fn get_country_codes(&self) -> Vec<&str> {
        self.country_tiles.keys().map(|s| s.as_str()).collect()
    }

    pub fn get_country_tile_count(&self, country_code: &str) -> usize {
        self.country_tiles.get(country_code).map_or(0, |v| v.len())
    }

    pub fn total_assigned_tiles(&self) -> usize {
        self.country_tiles.values().map(|v| v.len()).sum()
    }

    pub fn total_unassigned_tiles(&self) -> usize {
        self.unassigned_tiles.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &Vec<u32>)> {
        self.country_tiles.iter()
    }

    pub fn get_statistics(&self) -> CountryTilesStatistics {
        let mut stats = CountryTilesStatistics {
            total_tiles: self.total_assigned_tiles() + self.total_unassigned_tiles(),
            assigned_tiles: self.total_assigned_tiles(),
            unassigned_tiles: self.total_unassigned_tiles(),
            country_counts: HashMap::new(),
        };

        for (code, tiles) in &self.country_tiles {
            stats.country_counts.insert(code.clone(), tiles.len());
        }

        stats
    }
}

#[derive(Debug)]
pub struct CountryTilesStatistics {
    pub total_tiles: usize,
    pub assigned_tiles: usize,
    pub unassigned_tiles: usize,
    pub country_counts: HashMap<String, usize>,
}


impl RTreeObject for CountryPolygon {
    type Envelope = AABB<[f64; 2]>;

    fn envelope(&self) -> Self::Envelope {
        let mut min_x = f64::INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;

        for polygon in &self.coordinates {
            for &(x, y) in polygon {
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
        }

        AABB::from_corners([min_x, min_y], [max_x, max_y])
    }
}

impl PointDistance for CountryPolygon {
    fn distance_2(&self, point: &[f64; 2]) -> f64 {
        let envelope = self.envelope();
        envelope.distance_2(point)
    }
}

pub struct GeoLookup {
    rtree: RTree<CountryPolygon>,
}

impl GeoLookup {
    pub fn from_file(path: &str) -> Result<Self, Box<dyn Error + Send + Sync>> {
        let geojson_data = std::fs::read_to_string(path)?;
        Self::new(&geojson_data)
    }

    // Optional: A method that uses a default path
    pub fn from_default_location() -> Result<Self, Box<dyn Error + Send + Sync>> {
        Self::from_file("countries.geojson")
    }

    pub fn new(geojson_data: &str) -> Result<Self, Box<dyn Error + Send + Sync>> {
        let geojson = geojson_data.parse::<GeoJson>()?;
        let mut rtree = RTree::new();

        if let GeoJson::FeatureCollection(collection) = geojson {
            for mut feature in collection.features {
                if let Some(geometry) = feature.geometry.take() {
                    if let Value::MultiPolygon(multi_polygon) = geometry.value {
                        let country_code = feature
                            .property("ISO_A2")
                            .and_then(|p| p.as_str())
                            .unwrap_or("")
                            .to_string();

                        let name = feature
                            .property("ADMIN")
                            .and_then(|p| p.as_str())
                            .unwrap_or("")
                            .to_string();

                        let coordinates: Vec<Vec<(f64, f64)>> = multi_polygon
                            .into_iter()
                            .flat_map(|polygon| {
                                polygon.into_iter().map(|ring| {
                                    ring.into_iter()
                                        .map(|pos| (pos[0], pos[1]))
                                        .collect()
                                })
                            })
                            .collect();

                        rtree.insert(CountryPolygon {
                            country_code,
                            name,
                            coordinates,
                        });
                    }
                }
            }
        }

        Ok(Self { rtree })
    }

    pub fn uv_to_latlong(u: f64, v: f64) -> (f64, f64) {
        let longitude = (u - 0.5) * 360.0;
        let latitude = (v - 0.5) * 180.0;
        (longitude, latitude)
    }

    fn point_in_polygon(point: (f64, f64), polygon: &[(f64, f64)]) -> bool {
        let (x, y) = point;
        let mut inside = false;
        let mut j = polygon.len() - 1;

        for i in 0..polygon.len() {
            let (xi, yi) = polygon[i];
            let (xj, yj) = polygon[j];

            if ((yi > y) != (yj > y)) &&
                (x < (xj - xi) * (y - yi) / (yj - yi) + xi)
            {
                inside = !inside;
            }
            j = i;
        }

        inside
    }

    pub fn find_country(&self, tile: &TileVertex) -> Option<String> {
        let (longitude, latitude) = Self::uv_to_latlong(tile.uv.u, tile.uv.v);
        let point = [longitude, latitude];

        // Use nearest_neighbor_iter to find potential matches
        self.rtree
            .nearest_neighbor_iter(&point)
            .find(|country| {
                country.coordinates.iter().any(|polygon| {
                    Self::point_in_polygon((longitude, latitude), polygon)
                })
            })
            .map(|country| country.country_code.to_lowercase().clone())
    }
}