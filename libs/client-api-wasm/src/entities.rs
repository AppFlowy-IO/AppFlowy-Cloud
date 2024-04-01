use client_api::error::ErrorCode;
use serde::{Deserialize, Serialize};
use tsify::Tsify;
use wasm_bindgen::JsValue;

macro_rules! from_struct_for_jsvalue {
  ($type:ty) => {
    impl From<$type> for JsValue {
      fn from(value: $type) -> Self {
        JsValue::from_str(&serde_json::to_string(&value).unwrap())
      }
    }
  };
}

#[derive(Tsify, Serialize, Deserialize, Default, Debug)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct Configuration {
  pub compression_quality: u32,
  pub compression_buffer_size: usize,
}

#[derive(Tsify, Serialize, Deserialize, Default, Debug)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct ClientAPIConfig {
  pub base_url: String,
  pub ws_addr: String,
  pub gotrue_url: String,
  pub device_id: String,
  pub configuration: Option<Configuration>,
  pub client_id: String,
}

#[derive(Tsify, Serialize, Deserialize, Default, Debug)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct ClientResponse {
  pub code: ErrorCode,
  pub message: String,
}

from_struct_for_jsvalue!(ClientResponse);
