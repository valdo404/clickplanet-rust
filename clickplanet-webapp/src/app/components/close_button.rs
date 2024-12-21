use dioxus::prelude::*;


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