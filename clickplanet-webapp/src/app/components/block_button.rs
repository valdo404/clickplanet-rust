use dioxus::prelude::*;

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