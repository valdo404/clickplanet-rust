use glam::Vec3;
use image::GenericImageView;
use serde::{Deserialize, Serialize};
use std::env;
use std::process;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Coordinates {
    positions: Vec<f32>,
    uvs: Vec<f32>,
    length: usize,
}

fn main() {
    // Parse command-line arguments
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        eprintln!("Usage: {} <detail> <map_file_path> [threshold]", args[0]);
        process::exit(1);
    }

    let detail: u32 = args[1].parse().expect("Invalid detail level");
    let map_file_path = &args[2];
    let threshold: u8 = args.get(3)
        .map(|t| t.parse().expect("Invalid threshold"))
        .unwrap_or(128);

    // Load map
    let (pixels, width, height) = load_map(map_file_path);

    // Generate coordinates
    let base_coordinates = generate_base_coordinates(detail);
    let land_coordinates = filter_out_coordinates_not_on_land(
        &base_coordinates,
        &pixels,
        width,
        height,
        threshold
    );

    // Output as JSON
    println!("{}", serde_json::to_string(&land_coordinates).unwrap());
}

fn load_map(file_path: &str) -> (Vec<u8>, u32, u32) {
    let img = image::open(file_path).expect("Failed to open image");
    let (width, height) = img.dimensions();

    // Convert to RGB bytes
    let rgb_pixels: Vec<u8> = img.to_rgb8()
        .pixels()
        .flat_map(|p| p.0.to_vec())
        .collect();

    (rgb_pixels, width, height)
}

fn generate_base_coordinates(_detail: u32) -> Coordinates {
    // Simplified Icosahedron generation
    let golden_ratio = (1.0 + 5.0_f32.sqrt()) / 2.0;

    // 12 vertices of an icosahedron
    let vertices = vec![
        Vec3::new(-1.0, golden_ratio, 0.0),
        Vec3::new( 1.0, golden_ratio, 0.0),
        Vec3::new(-1.0, -golden_ratio, 0.0),
        Vec3::new( 1.0, -golden_ratio, 0.0),

        Vec3::new(0.0, -1.0, golden_ratio),
        Vec3::new(0.0, 1.0, golden_ratio),
        Vec3::new(0.0, -1.0, -golden_ratio),
        Vec3::new(0.0, 1.0, -golden_ratio),

        Vec3::new(golden_ratio, 0.0, -1.0),
        Vec3::new(golden_ratio, 0.0, 1.0),
        Vec3::new(-golden_ratio, 0.0, -1.0),
        Vec3::new(-golden_ratio, 0.0, 1.0),
    ];

    // Normalize vertices to unit sphere
    let vertices: Vec<Vec3> = vertices.iter()
        .map(|v| v.normalize())
        .collect();

    // Generate UVs using spherical projection
    let mut positions: Vec<f32> = Vec::new();
    let mut uvs = Vec::new();
    let mut unique_vertices = std::collections::HashMap::new();

    for vertex in vertices {
        let key = format!("{:.6},{:.6},{:.6}", vertex.x, vertex.y, vertex.z);

        if !unique_vertices.contains_key(&key) {
            // Compute UV using spherical projection
            let u = 0.5 + vertex.z.atan2(vertex.x) / (2.0 * std::f32::consts::PI);
            let v = 0.5 - vertex.y.asin() / std::f32::consts::PI;

            positions.extend_from_slice(&[vertex.x, vertex.y, vertex.z]);
            uvs.extend_from_slice(&[u, v]);

            unique_vertices.insert(key, unique_vertices.len());
        }
    }

    Coordinates {
        positions: positions.clone(),
        uvs,
        length: positions.len() / 3,
    }
}

fn filter_out_coordinates_not_on_land(
    coordinates: &Coordinates,
    pixels: &[u8],
    width: u32,
    height: u32,
    threshold: u8
) -> Coordinates {
    let mut new_positions = Vec::new();
    let mut new_uvs = Vec::new();

    for i in 0..coordinates.length {
        let u = coordinates.uvs[i * 2];
        let v = coordinates.uvs[i * 2 + 1];

        if is_land(u, v, pixels, width, height, threshold) {
            new_positions.push(coordinates.positions[i * 3]);
            new_positions.push(coordinates.positions[i * 3 + 1]);
            new_positions.push(coordinates.positions[i * 3 + 2]);

            new_uvs.push(u);
            new_uvs.push(v);
        }
    }

    Coordinates {
        positions: new_positions.clone(),
        uvs: new_uvs,
        length: new_positions.len() / 3,
    }
}

fn is_land(
    u: f32,
    v: f32,
    pixels: &[u8],
    width: u32,
    height: u32,
    threshold: u8
) -> bool {
    let x = (u * width as f32).floor() as usize;
    let y = (v * height as f32).floor() as usize;

    // Ensure we're within image bounds
    if x >= width as usize || y >= height as usize {
        return false;
    }

    let index = (y * width as usize + x) * 3;
    pixels[index] < threshold
}