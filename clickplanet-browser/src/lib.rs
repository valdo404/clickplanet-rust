
use wasm_bindgen::prelude::*;
use web_sys::{WebSocket, MessageEvent};
use wasm_bindgen::JsCast;
use js_sys::{Uint8Array, ArrayBuffer};
use clickplanet_proto::clicks::UpdateNotification;
use prost::Message;

#[wasm_bindgen(start)]
pub fn main() -> Result<(), JsValue> {
    // Connect to WebSocket server
    let ws = WebSocket::new("wss://clickplanet.lol/v2/ws/listen")?;

    // Set binary type to ArrayBuffer
    ws.set_binary_type(web_sys::BinaryType::Arraybuffer);

    // Create message handler
    let onmessage_callback = Closure::wrap(Box::new(move |e: MessageEvent| {
        if let Ok(buffer) = e.data().dyn_into::<ArrayBuffer>() {
            let uint8_array = Uint8Array::new(&buffer);
            let data: Vec<u8> = uint8_array.to_vec();

            // Decode protobuf message
            match UpdateNotification:: decode(data.as_slice()) {
                Ok(notification) => {
                    // Convert notification to JSON string for display
                    let display_text = format!("Received notification: {:?}", notification);
                    web_sys::console::log_1(&display_text.clone().into());

                    // Update UI
                    if let Some(window) = web_sys::window() {
                        if let Some(document) = window.document() {
                            let output_div = match document.get_element_by_id("output") {
                                Some(div) => div,
                                None => {
                                    let div = document.create_element("div").unwrap();
                                    div.set_id("output");
                                    document.body().unwrap().append_child(&div).unwrap();
                                    div
                                }
                            };

                            let msg_p = document.create_element("p").unwrap();
                            msg_p.set_text_content(Some(&display_text));
                            output_div.append_child(&msg_p).unwrap();
                        }
                    }
                },
                Err(e) => {
                    web_sys::console::error_1(&format!("Error decoding protobuf message: {}", e).into());
                }
            }
        } else {
            web_sys::console::error_1(&"Received non-binary message".into());
        }
    }) as Box<dyn FnMut(MessageEvent)>);

    // Set message handler
    ws.set_onmessage(Some(onmessage_callback.as_ref().unchecked_ref()));
    onmessage_callback.forget();

    Ok(())
}