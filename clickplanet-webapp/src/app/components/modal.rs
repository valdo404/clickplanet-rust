use dioxus::prelude::*;


#[derive(Props, Clone, PartialEq)]
pub struct ModalProps {
    #[props(optional)]
    pub title: Option<String>,             // Optional title for the modal
    pub children: Element,                // Modal content
    pub on_close: Callback<()>,           // Callback for when the modal is closed
    #[props(optional)]
    pub close_button_text: Option<String>, // Optional text for the close button
}

#[component]
pub fn Modal(props: ModalProps) -> Element {
    rsx!(
        div {
            class: "modal",
            onclick: move |_| props.on_close.call(()), // Close the modal when clicking outside

            div {
                class: "modal-content",
                onclick: move |evt| evt.stop_propagation(),

                match &props.title {
                    Some(title) => rsx!(
                        div {
                            class: "modal-header",
                            h2 { "{title}" }
                        }
                    ),
                    None => rsx!()
                },

                {
                    props.children
                },

                CloseButton {
                    text: props.close_button_text.clone(),
                    on_click: move |_| props.on_close.call(()),
                }
            }
        }
    )
}