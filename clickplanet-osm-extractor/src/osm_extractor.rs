use osmpbf::{Blob, BlobDecode, BlobReader, Node, TagIter, Way};
use postgres::{Client, NoTls};
use serde_json;
use std::error::Error;
use clap::Parser;

struct City {
    id: i64,
    name: Option<String>,
    lat: f64,
    lon: f64,
    population: Option<i64>,
}

struct Road {
    id: i64,
    name: Option<String>,
    nodes: Vec<i64>, // List of node IDs forming the road
}


#[derive(Parser)]
#[command(name = "OSM Extractor")]
#[command(author = "Laurent Valdes")]
#[command(version = "1.0")]
#[command(about = "Extract data from OpenStreetMap and save it to a Postgres/PostGIS database")]
struct Args {
    #[arg(short, long)]
    osm_file: String,

    #[arg(short, long)]
    db_url: String,
}


/// Entry point
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    extract_osm_to_db(&args.osm_file, &args.db_url)?;

    println!("Extraction complete. Data saved to Postgres/PostGIS database.");
    Ok(())
}


fn extract_osm_to_db(osm_file: &str, db_url: &str) -> Result<(), Box<dyn std::error::Error>> {
    // Establish a Postgres connection
    let mut client = Client::connect(db_url, NoTls)?;

    // Create the required tables
    create_cities_table(&mut client)?;
    create_roads_table(&mut client)?;

    // Open and process the OSM file
    let reader = BlobReader::from_path(osm_file)?;

    for blob in reader {
        let blob: Blob = blob?;

        decode_blob(&mut client, blob)?;
    }

    Ok(())
}


fn decode_blob(client: &mut Client, blob: Blob) -> Result<(), Box<dyn Error>> {
    if let BlobDecode::OsmData(block) = blob.decode()? {
        for group in block.groups() {
            // Process cities (nodes with "place=city" or "place=town")
            for node in group.nodes() {
                decode_node(client, node)?;
            }

            // Process roads (ways with "highway" tag)
            for way in group.ways() {
                decode_way(client, way)?;
            }
        }
    }
    Ok(())
}

fn decode_way(client: &mut Client, way: Way) -> Result<(), Box<dyn Error>> {
    let mut tags: TagIter = way.tags();

    if tags.any(|(key, _)| key == "highway") {
        let mut name = None;
        let nodes = way.refs().collect::<Vec<i64>>();

        for (key, value) in way.tags() { // Iterate using tuple destructuring
            if key == "name" {
                name = Some(value.to_string());
                break;
            }
        }

        let road = Road {
            id: way.id(),
            name,
            nodes,
        };

        insert_road(client, &road)?;
    };


    Ok(())
}



fn decode_node(client: &mut Client, node: Node) -> Result<(), Box<dyn Error>> {
    let tags = node.tags();

    let mut place_tag = None;
    let mut name_tag = None;
    let mut population_tag = None;

    for (key, val) in tags {
        match key {
            "place" => place_tag = Some(val),
            "name" => name_tag = Some(val),
            "population" => population_tag = Some(val),
            _ => {}, // Ignore other tags
        }
    }

    if let (Some(place), Some(name)) = (place_tag, name_tag) {
        if place == "city" || place == "town" {
            let city = City {
                id: node.id(),
                name: Some(name.to_string()),
                lat: node.lat(),
                lon: node.lon(),
                population: population_tag.and_then(|p| p.parse().ok()),
            };

            insert_city(client, &city)?;
        }
    }

    Ok(())
}


fn create_roads_table(client: &mut Client) -> Result<(), Box<dyn Error>> {
    client.execute(
        "CREATE TABLE IF NOT EXISTS roads (
            road_id BIGINT PRIMARY KEY,
            name TEXT,
            nodes JSONB
        )",
        &[],
    )?;
    Ok(())
}

fn create_cities_table(client: &mut Client) -> Result<(), Box<dyn Error>> {
    client.execute(
        "CREATE TABLE IF NOT EXISTS cities (
            city_id BIGINT PRIMARY KEY,
            name TEXT,
            latitude DOUBLE PRECISION,
            longitude DOUBLE PRECISION,
            population BIGINT,
            geom GEOGRAPHY(Point, 4326)
        )",
        &[],
    )?;
    Ok(())
}

fn insert_road(client: &mut Client, road: &Road) -> Result<(), Box<dyn Error>> {
    client.query(
        "INSERT INTO roads (road_id, name, nodes)
                                 VALUES ($1, $2, $3)",
        &[
            &road.id,
            &road.name,
            &serde_json::to_string(&road.nodes)?,
        ],
    )?;

    Ok(())
}

fn insert_city(client: &mut Client, city: &City) -> Result<(), Box<dyn Error>> {
    client.execute(
        "INSERT INTO cities (city_id, name, latitude, longitude, population, geom)
                                     VALUES ($1, $2, $3, $4, $5, ST_SetSRID(ST_MakePoint($3, $4), 4326))",
        &[
            &city.id,
            &city.name,
            &city.lat,
            &city.lon,
            &city.population,
        ],
    )?;
    Ok(())
}
