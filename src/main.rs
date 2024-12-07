mod client;

use prost::Message;
use tokio_tungstenite::connect_async;
use futures::StreamExt;

pub mod clicks {
    include!(concat!(env!("OUT_DIR"), "/clicks.v1.rs"));
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = client::Client::new("clickplanet.lol");

    // Get the stream of updates
    let mut updates = client.listen_for_updates().await?;

    println!("Connected to WebSocket, waiting for updates...");

    // Process the stream
    while let Some(update) = updates.next().await {
        println!("Update: tile={}, from={}, to={}",
                 update.tile_id,
                 update.previous_country_id,
                 update.country_id
        );
    }

    Ok(())
}
