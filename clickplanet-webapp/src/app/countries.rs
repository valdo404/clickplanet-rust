use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use wasm_bindgen::prelude::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Country {
    pub name: String,
    pub code: String,
}

#[wasm_bindgen]
pub struct CountriesMap {
    countries: HashMap<String, Country>,
}

#[wasm_bindgen]
impl CountriesMap {
    #[wasm_bindgen(constructor)]
    pub fn new(data: JsValue) -> CountriesMap {
        let countries_data: HashMap<String, String> = serde_wasm_bindgen::from_value(data)
            .map_err(|e| js_sys::Error::new(&e.to_string()))
            .unwrap_throw();

        let countries: HashMap<String, Country> = countries_data
            .into_iter()
            .map(|(code, name)| {
                (code.clone(), Country {
                    name,
                    code,
                })
            })
            .collect();

        CountriesMap { countries }
    }

    pub fn get(&self, code: &str) -> JsValue {
        match self.countries.get(code) {
            Some(country) => serde_wasm_bindgen::to_value(country).unwrap(),
            None => JsValue::null(),
        }
    }

    pub fn contains_key(&self, code: &str) -> bool {
        self.countries.contains_key(code)
    }

    pub fn keys(&self) -> Vec<String> {
        self.countries.keys().cloned().collect()
    }

    pub fn len(&self) -> usize {
        self.countries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.countries.is_empty()
    }
}