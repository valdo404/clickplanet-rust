use dioxus::prelude::*;
use crate::app::components::modal::Modal;

#[derive(Props, Clone, PartialEq)]
pub struct OnLoadModalProps {
    title: String,              // Title for the Modal
    children: Element,       // Children of the Modal
}

#[component]
pub fn OnLoadModal(props: OnLoadModalProps) -> Element {
    let mut is_open = use_signal(|| true); // Local state to track modal visibility

    if is_open() {
        rsx!(
            div { class: "modal-onload",
                Modal {
                    on_close: move || is_open.set(false),
                    title: props.title.to_string(),
                    children: props.children.clone(),
                }
            }
        )
    } else {
        rsx!()
    }

}