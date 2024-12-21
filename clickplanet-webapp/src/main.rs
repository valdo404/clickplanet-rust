use dioxus::prelude::*;
use crate::app::components::block_button::BlockButton;
use crate::app::components::close_button::CloseButton;
use crate::app::components::discord_button::DiscordButton;

mod app {
    pub mod components {
        pub mod buy_me_a_coffee;
        pub mod block_button;
        pub mod close_button;
        pub mod discord_button;
        pub mod modal;
        pub mod modal_manager;
        pub mod on_load_modal;
    }
}

fn main() {
    console_log::init_with_level(log::Level::Debug).expect("Unable to initialize console_log");

    dioxus::launch(App);
}

#[component]
fn App() -> Element {
    rsx! {
        app::components::buy_me_a_coffee::BuyMeACoffee { }
        BlockButton {
            text: "Click Me".to_string(),
            image_url: "".to_string(),
            class_name: Some("my-class".to_string()),
            on_click: move |_evt| {
                log::info!("Button clicked!");
            }
        }
        DiscordButton {
            message: Some("Join our Discord server".to_string()),
        }
    }
}
