use serde::Serialize;
use wasm_bindgen::prelude::*;

#[derive(Serialize)]
pub struct ExtractResult {
    pub modified: String,
    pub dynamic: String,
    pub func_ids: Vec<String>,
}

#[wasm_bindgen]
pub fn extract(source: &str, path: &str) -> JsValue {
    let (modified, dynamic, func_ids) = sugg_ast::extract_dynamics(source, path);
    serde_wasm_bindgen::to_value(&ExtractResult {
        modified,
        dynamic,
        func_ids,
    })
    .unwrap_or(JsValue::UNDEFINED)
}

#[wasm_bindgen]
pub fn analyze_apis(dynamic_js: &str) -> JsValue {
    let usages = sugg_ast::analyze_dynamic_apis(dynamic_js);
    serde_wasm_bindgen::to_value(&usages).unwrap_or(JsValue::UNDEFINED)
}
