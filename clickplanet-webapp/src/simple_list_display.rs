use wasm_bindgen::prelude::*;
use web_sys::{Document, Element};

#[wasm_bindgen]
pub struct SimpleListDisplay {
    output_div: Element,
}

#[wasm_bindgen]
impl SimpleListDisplay {
    #[wasm_bindgen(constructor)]
    pub fn new(document: &Document) -> Result<SimpleListDisplay, JsValue> {
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

        Ok(SimpleListDisplay { output_div })
    }

    #[wasm_bindgen]
    pub fn add_message(&self, message: &str) -> Result<(), JsValue> {
        let document = self.output_div
            .owner_document()
            .ok_or_else(|| JsValue::from_str("No owner document"))?;

        let msg_p = document.create_element("p")?;
        msg_p.set_text_content(Some(message));
        self.output_div.append_child(&msg_p)?;

        Ok(())
    }
}