use wasm_bindgen::prelude::*;
use web_sys::Element;

#[wasm_bindgen]
#[derive(Clone)]
pub struct DomDisplay(Element);

#[wasm_bindgen]
impl DomDisplay {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<DomDisplay, JsValue> {
        let window = web_sys::window().unwrap();
        let document = window.document().unwrap();

        let output_div = match document.get_element_by_id("output") {
            Some(div) => div,
            None => {
                let div = document.create_element("div")?;
                div.set_id("output");
                document.body()
                    .ok_or_else(|| JsValue::from_str("No body element found"))?
                    .append_child(&div)?;
                div
            }
        };

        Ok(DomDisplay(output_div))
    }

    #[wasm_bindgen]
    pub fn add_message(&self, message: &str) -> Result<(), JsValue> {
        let document = self.0
            .owner_document()
            .ok_or_else(|| JsValue::from_str("No owner document"))?;

        let msg_p = document.create_element("p")?;
        msg_p.set_text_content(Some(message));
        self.0.append_child(&msg_p)?;

        Ok(())
    }
}