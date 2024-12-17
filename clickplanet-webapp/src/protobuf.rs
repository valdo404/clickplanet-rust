use clickplanet_proto::clicks::UpdateNotification;
use prost::Message;
use serde::Serialize;
use serde_wasm_bindgen::to_value;
use wasm_bindgen::prelude::*;

#[derive(Serialize)]
struct NotificationWrapper<'a> {
    tile_id: i32,
    country_id: &'a str,
    previous_country_id: &'a str,
}

#[wasm_bindgen]
pub fn decode_update_notification(data: Vec<u8>) -> Result<JsValue, JsValue> {
    match UpdateNotification::decode(&data[..]) {
        Ok(notification) => {
            let wrapper = NotificationWrapper {
                tile_id: notification.tile_id,
                country_id: &notification.country_id,
                previous_country_id: &notification.previous_country_id,
            };

            Ok(to_value(&wrapper)
                .map_err(|e| JsValue::from_str(&format!("JS conversion error: {}", e)))?)
        },

        Err(e) => Err(JsValue::from_str(&format!("Error decoding protobuf message: {}", e)))
    }
}