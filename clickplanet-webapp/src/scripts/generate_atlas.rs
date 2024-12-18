use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use anyhow::{Context, Result};
use image::{DynamicImage, ImageBuffer, Rgba, RgbaImage};
use serde_json::{json, Value};

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    let sprites_dir = args.get(1)
        .cloned()
        .unwrap_or_else(|| "./static/countries/png100px".to_string());
    let dest_dir = args.get(2)
        .cloned()
        .unwrap_or_else(|| "./static/countries".to_string());

    fs::create_dir_all(&dest_dir)?;

    let png_files: Vec<PathBuf> = fs::read_dir(&sprites_dir)?
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry.path()
                .extension()
                .map_or(false, |ext| ext.eq_ignore_ascii_case("png"))
        })
        .map(|entry| entry.path())
        .collect();

    let (atlas_image, coordinates) = generate_sprite_atlas(&png_files)?;

    let atlas_path = Path::new(&dest_dir).join("atlas.png");
    let json_path = Path::new(&dest_dir).join("atlas.json");

    atlas_image.save(&atlas_path)
        .context("Failed to save atlas image")?;

    let coordinates_json = serde_json::to_string_pretty(&coordinates)
        .context("Failed to serialize coordinates")?;
    fs::write(json_path, coordinates_json)
        .context("Failed to write coordinates JSON")?;

    println!("Atlas generated");
    Ok(())
}

fn generate_sprite_atlas(sprite_paths: &[PathBuf]) -> Result<(DynamicImage, Value)> {
    let mut total_width = 0;
    let mut max_height = 0;
    let mut loaded_images = Vec::new();

    for path in sprite_paths {
        let img = image::open(path)?;
        total_width += img.width();
        max_height = max_height.max(img.height());
        loaded_images.push(img);
    }

    let mut atlas = RgbaImage::new(total_width, max_height);

    let mut coordinates = serde_json::Map::new();
    let mut current_x = 0;

    for (index, img) in loaded_images.iter().enumerate() {
        let file_name = sprite_paths[index]
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");

        let mut coord_entry = serde_json::Map::new();
        coord_entry.insert("x".to_string(), json!(current_x));
        coord_entry.insert("y".to_string(), json!(0));
        coord_entry.insert("width".to_string(), json!(img.width()));
        coord_entry.insert("height".to_string(), json!(img.height()));

        for (x, y, pixel) in img.to_rgba8().enumerate_pixels() {
            if current_x + x < total_width {
                atlas.put_pixel(current_x + x, y, *pixel);
            }
        }

        coordinates.insert(file_name.to_string(), json!(coord_entry));
        current_x += img.width();
    }

    Ok((DynamicImage::ImageRgba8(atlas), json!(coordinates)))
}