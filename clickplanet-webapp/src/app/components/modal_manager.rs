use dioxus::prelude::*;
use crate::app::components::block_button::BlockButtonProps;
use crate::app::components::modal::Modal;
use crate::app::components::block_button::BlockButton;

#[derive(Props, Clone, PartialEq)]
pub struct ModalManagerProps {
    pub open_by_default: bool,          // Whether the modal is open by default
    pub modal_title: String,           // Title of the modal
    pub modal_children: Element,       // Children content inside the modal
    pub button_props: BlockButtonProps, // Props for the BlockButton
    #[props(optional)]
    pub close_button_text: Option<String>, // Optional close button text
}

#[component]
pub fn ModalManager(props: ModalManagerProps) -> Element {
    let is_open = use_signal(|| props.open_by_default);

    // Toggles the modal on/off
    let toggle_popup = {
        let is_open = is_open.clone();
        move |val: bool| {
            let mut is_open = is_open.clone(); // Clone the signal state
            move || {
                is_open.set(val); // Set the value directly without arguments
            }
        }
    };

    rsx!(
        div { // Add a wrapper `div` for all the elements, as `rsx!` requires a single root container
            // BlockButton to control the modal
            BlockButton {
                on_click: move |evt| {
                    props.button_props.on_click.call(evt); // Trigger the button's custom click callback
                    (toggle_popup(true))(); // Open the modal
                },
                text: props.button_props.text.clone(),
                image_url: props.button_props.image_url.clone(),
                class_name: props.button_props.class_name.clone(),
            },

            // modal content - only show when `is_open` is true
            {
                if *is_open.read() {
                    rsx!(
                        Modal {
                            title: props.modal_title.clone(),
                            children: props.modal_children.clone(), // Pass the modal's content
                            on_close: toggle_popup(false), // Close the modal
                            close_button_text: props.close_button_text.clone(),
                        }
                    )
                } else {
                    rsx!() // Render nothing when modal is closed
                }
            }
        }
    )
}