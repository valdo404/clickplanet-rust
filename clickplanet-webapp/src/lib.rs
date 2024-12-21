mod simple_list_display;
mod websocket;

mod app {
    mod countries;
}

mod backends {
    mod backend;
    mod fake_backend;
    mod http_backend;
}

use wasm_bindgen::prelude::*;
use crate::simple_list_display::DomDisplay;
use crate::websocket::WebSocketWrapper;

#[wasm_bindgen(start)]
pub fn main() -> Result<(), JsValue> {
    let display = DomDisplay::new()?;

    // Create the WebSocket
    let protocol = "wss";
    let hostname = "clickplanet.lol";
    let port = 443;
    let ws_url = format!("{}://{}:{}/v2/ws/listen", protocol, hostname, port);

    let ws = WebSocketWrapper::new(&ws_url)?;

    // Create message handler
    let display_ref = display.clone();
    let callback = Closure::wrap(Box::new(move |message: String| {
        if let Err(e) = display_ref.add_message(&message) {
            web_sys::console::error_1(&format!("Error displaying message: {:?}", e).into());
        }
    }) as Box<dyn FnMut(String)>);

    ws.set_message_handler(&callback.as_ref().unchecked_ref())?;

    callback.forget();

    Ok(())
}