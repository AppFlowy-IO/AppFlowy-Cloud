use web_sys::wasm_bindgen::JsValue;
use crate::WSError;

impl From<JsValue> for WSError {
    fn from(value: JsValue) -> Self {
        WSError::WasmError(value)
    }
}
