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

#[derive(Props, Clone, PartialEq)]
pub struct CloseButtonProps {
    pub on_click: Callback<MouseEvent, ()>,
    #[props(optional)]
    pub class_name: Option<String>,
    #[props(optional)]
    pub text: Option<String>,
}
#[component]
pub fn CloseButton(props: CloseButtonProps) -> Element {
    let class_name = match &props.class_name {
        Some(class) => format!("button button-close {}", class),
        None => "button button-close".to_string(),
    };

    rsx!(
        button {
            class: "{class_name}",
            onclick: move |evt| props.on_click.call(evt),
            "{props.text.clone().unwrap_or_else(|| String::from(\"Close\"))}"
        }
    )
}

#[derive(Props, Clone, PartialEq)]
pub struct DiscordButtonProps {
    #[props(optional)]
    pub message: Option<String>, // Optional message to display, defaults to "Join us on Discord"
}

#[component]
pub fn DiscordButton(props: DiscordButtonProps) -> Element {
    let message = props
        .message
        .clone()
        .unwrap_or_else(|| "Join us on Discord".to_string());

    rsx!(
        a {
            href: "https://discord.gg/Nwekj6ndbn",
            target: "_blank",
            rel: "noopener noreferrer",
            class: "button button-discord",
            aria_label: "{message}",
            "{message}"
        }
    )
}