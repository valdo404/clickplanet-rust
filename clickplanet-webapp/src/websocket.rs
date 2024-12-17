use wasm_bindgen::prelude::*;
use web_sys::{WebSocket, MessageEvent, CloseEvent};
use wasm_bindgen::JsCast;
use js_sys::{Uint8Array, ArrayBuffer, Function};
use clickplanet_proto::clicks::UpdateNotification;
use prost::Message;
use std::cell::Cell;
use std::rc::Rc;
use wasm_bindgen::closure::Closure;

#[wasm_bindgen]
pub struct ClickplanetWebsocketClient {
    ws: WebSocket,
    _message_callback: Closure<dyn FnMut(MessageEvent)>,
    reconnect_attempt: Rc<Cell<u32>>,
    config: ConnectionConfig,
}

#[derive(Clone)]
struct ConnectionConfig {
    secure: bool,
    hostname: String,
    port: u16,
    on_message: Function,
}

impl ClickplanetWebsocketClient {
    fn connect(
        config: ConnectionConfig,
        reconnect_attempt: Rc<Cell<u32>>,
    ) -> Result<(WebSocket, Closure<dyn FnMut(MessageEvent)>), JsValue> {
        let protocol = if config.secure { "wss" } else { "ws" };
        let ws_url = format!("{}://{}:{}/v2/ws/listen", protocol, config.hostname, config.port);

        let ws = WebSocket::new(&ws_url)?;
        ws.set_binary_type(web_sys::BinaryType::Arraybuffer);

        let on_message = config.on_message.clone();
        let message_callback = Closure::wrap(Box::new(move |e: MessageEvent| {
            if let Ok(buffer) = e.data().dyn_into::<ArrayBuffer>() {
                let uint8_array = Uint8Array::new(&buffer);
                let data: Vec<u8> = uint8_array.to_vec();

                match UpdateNotification::decode(data.as_slice()) {
                    Ok(notification) => {
                        let _ = on_message.call1(
                            &JsValue::NULL,
                            &JsValue::from_str(&format!("{:?}", notification))
                        );
                    },
                    Err(e) => {
                        web_sys::console::error_1(&format!("Error decoding protobuf message: {}", e).into());
                    }
                }
            } else {
                web_sys::console::error_1(&"Received non-binary message".into());
            }
        }) as Box<dyn FnMut(MessageEvent)>);

        let config_clone = config.clone();
        let reconnect_attempt_clone = reconnect_attempt.clone();

        let close_callback = Closure::wrap(Box::new(move |e: CloseEvent| {
            if e.code() != 1000 {
                let current_attempt = reconnect_attempt_clone.get();
                let delay = (1000 * 2_u32.pow(current_attempt.min(4))) as i32;

                let window = web_sys::window().unwrap();
                let reconnect_attempt = reconnect_attempt_clone.clone();
                let config = config_clone.clone();

                let reconnect_closure = Closure::once(move || {
                    match Self::connect(config, reconnect_attempt.clone()) {
                        Ok(_) => {
                            web_sys::console::log_1(&"Reconnected successfully".into());
                            reconnect_attempt.set(0);
                        },
                        Err(e) => {
                            web_sys::console::error_1(&format!("Reconnection failed: {:?}", e).into());
                            reconnect_attempt.set(current_attempt + 1);
                        }
                    }
                });

                let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(
                    reconnect_closure.as_ref().unchecked_ref::<Function>(),
                    delay,
                );
                reconnect_closure.forget();
            }
        }) as Box<dyn FnMut(CloseEvent)>);

        ws.set_onmessage(Some(message_callback.as_ref().unchecked_ref()));
        ws.set_onclose(Some(close_callback.as_ref().unchecked_ref()));

        Ok((ws, message_callback))
    }
}

#[wasm_bindgen]
impl ClickplanetWebsocketClient {
    #[wasm_bindgen(constructor)]
    pub fn new(
        secure: bool,
        hostname: &str,
        port: u16,
        on_message: Function
    ) -> Result<ClickplanetWebsocketClient, JsValue> {
        let config = ConnectionConfig {
            secure,
            hostname: hostname.to_string(),
            port,
            on_message
        };

        let reconnect_attempt = Rc::new(Cell::new(0));

        let (ws, message_callback) =
            Self::connect(config.clone(), reconnect_attempt.clone())?;  // Changed from &config to config.clone()


        Ok(ClickplanetWebsocketClient {
            ws,
            _message_callback: message_callback,
            reconnect_attempt,
            config,
        })
    }

    #[wasm_bindgen]
    pub fn close(&self) -> Result<(), JsValue> {
        self.ws.close()
    }

    #[wasm_bindgen]
    pub fn get_ready_state(&self) -> u16 {
        self.ws.ready_state()
    }
}