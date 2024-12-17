mod websocket;
mod simple_display;

pub use simple_display::MessageDisplay;
use js_sys::Function;
use std::cell::RefCell;
use wasm_bindgen::prelude::*;
use web_sys::window;
pub use websocket::ClickplanetWebsocketClient;

thread_local! {
    static DISPLAY: RefCell<Option<MessageDisplay>> = RefCell::new(None);
}

#[wasm_bindgen(start)]
pub fn main() -> Result<(), JsValue> {
    let window = window().expect("no global window exists");
    let document = window.document().expect("no document on window");

    let display = MessageDisplay::new(&document)?;
    DISPLAY.with(|d| {
        *d.borrow_mut() = Some(display);
    });

    let callback = Closure::wrap(Box::new(move |message: String| {
        DISPLAY.with(|display| {
            if let Some(display) = display.borrow().as_ref() {
                if let Err(e) = display.add_message(&message) {
                    web_sys::console::error_1(&format!("Error displaying message: {:?}", e).into());
                }
            }
        });
    }) as Box<dyn FnMut(String)>);

    let message_fn = callback.as_ref().unchecked_ref::<Function>().clone();


    let client = ClickplanetWebsocketClient::new(
        true,                    // secure (wss://)
        "clickplanet.lol",       // hostname
        443,                     // port
        message_fn,
    )?;

    std::mem::forget(client);
    std::mem::forget(callback);

    Ok(())
}