mod client;

use futures::StreamExt;


#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = client::Client::new("clickplanet.lol");

    println!("Getting ownerships:");

    let ownerships = client.get_ownerships().await?;

    println!("Initial ownerships:");

    for ownership in ownerships.ownerships {
        println!(
            "Tile {} owned by {}",
            ownership.tile_id, ownership.country_id
        );
    }

    println!("\nConnecting to WebSocket for live updates...");

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
