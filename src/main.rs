use prost::Message;
use tokio_tungstenite::connect_async;
use futures::StreamExt;

pub mod clicks {
    include!(concat!(env!("OUT_DIR"), "/clicks.v1.rs"));
}

#[tokio::main]
async fn main() {
    // Example WebSocket connection
    let url = "wss://clickplanet.lol/ws/listen";
    let (ws_stream, _) = connect_async(url).await.expect("Failed to connect");
    let (_, read) = ws_stream.split();

    // Handle incoming messages
    read.for_each(|message| async {
        if let Ok(msg) = message {
            if let Ok(update) = clicks::UpdateNotification::decode(msg.into_data().as_slice()) {
                println!("Update: tile={}, from={}, to={}", 
                    update.tile_id, 
                    update.previous_country_id, 
                    update.country_id);
            }
        }
    }).await;
}
