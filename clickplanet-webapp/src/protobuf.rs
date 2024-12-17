use wasm_bindgen::prelude::*;
use clickplanet_proto::clicks::UpdateNotification;
use prost::Message;

#[wasm_bindgen]
pub fn decode_update_notification(data: Vec<u8>) -> Result<String, JsValue> {
    match UpdateNotification::decode(&data[..]) {
        Ok(notification) => Ok(format!("{:?}", notification)),
        Err(e) => Err(JsValue::from_str(&format!("Error decoding protobuf message: {}", e)))
    }
}