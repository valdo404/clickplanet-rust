use dioxus::prelude::*;

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
