use dioxus::prelude::*;

fn main() {
    console_log::init_with_level(log::Level::Debug).expect("Unable to initialize console_log");

    dioxus::launch(App);
}

#[component]
fn App() -> Element {
    rsx! {
        BuyMeACoffee { }
        BlockButton {
            text: "Click Me".to_string(),
            image_url: "".to_string(),
            class_name: Some("my-class".to_string()),
            on_click: move |_evt| {
                log::info!("Button clicked!");
            }
        }
    }
}

#[component]
pub fn BuyMeACoffee() -> Element {
    rsx!(
        a {
            href: "https://buymeacoffee.com/raphoester",
            target: "_blank",
            rel: "noopener noreferrer",
            class: "button button-coffee",
            "Buy me a coffee"
        }
    )
}


#[derive(Props, Clone, PartialEq)]
pub struct BlockButtonProps {
    on_click: Callback<MouseEvent, ()>,
    text: String,                 // Text to display on the button
    image_url: String,    // Optional image URL (not used in the current implementation)
    class_name: Option<String>,   // Optional class name for styling
}

#[component]
pub fn BlockButton(props: BlockButtonProps) -> Element {
    let class_name = match props.class_name {
        Some(class) => format!("button {}", class),
        None => "button".to_string(),
    };

    rsx!(
        button {
            class: "{class_name}",
            onclick: move |evt| props.on_click.call(evt), // Call the callback
            "{props.text}"
        })
}