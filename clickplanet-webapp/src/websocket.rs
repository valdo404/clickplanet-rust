use wasm_bindgen::prelude::*;
use web_sys::{WebSocket, MessageEvent};
use wasm_bindgen::JsCast;
use js_sys::{Uint8Array, ArrayBuffer, Function};
use clickplanet_proto::clicks::UpdateNotification;
use prost::Message;

#[wasm_bindgen]
pub struct WebSocketWrapper(WebSocket);

#[wasm_bindgen]
impl WebSocketWrapper {
    #[wasm_bindgen(constructor)]
    pub fn new(url: &str) -> Result<WebSocketWrapper, JsValue> {
        let ws = WebSocket::new(url)?;
        ws.set_binary_type(web_sys::BinaryType::Arraybuffer);
        Ok(WebSocketWrapper(ws))
    }

    #[wasm_bindgen]
    pub fn set_message_handler(&self, callback: &Function) -> Result<(), JsValue> {
        let cb_clone = callback.clone();
        let onmessage_fn = Closure::wrap(Box::new(move |e: MessageEvent| {
            if let Ok(buffer) = e.data().dyn_into::<ArrayBuffer>() {
                let uint8_array = Uint8Array::new(&buffer);
                let data: Vec<u8> = uint8_array.to_vec();

                if let Ok(notification) = UpdateNotification::decode(data.as_slice()) {
                    let _ = cb_clone.call1(
                        &JsValue::NULL,
                        &JsValue::from_str(&format!("{:?}", notification))
                    );
                }
            }
        }) as Box<dyn FnMut(MessageEvent)>);

        self.0.set_onmessage(Some(onmessage_fn.as_ref().unchecked_ref()));
        onmessage_fn.forget();
        Ok(())
    }
}