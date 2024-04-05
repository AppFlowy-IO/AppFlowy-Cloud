use client_api::entity::AFUserProfile;
use client_api::error::{AppResponseError, ErrorCode};
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
impl From<AppResponseError> for ClientResponse {
  fn from(err: AppResponseError) -> Self {
    ClientResponse {
      code: err.code,
      message: err.message.to_string(),
    }
  }
}

#[derive(Tsify, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct User {
  pub uid: i64,
  pub uuid: String,
  pub email: Option<String>,
  pub name: Option<String>,
  pub latest_workspace_id: String,
}

from_struct_for_jsvalue!(User);

impl From<AFUserProfile> for User {
  fn from(profile: AFUserProfile) -> Self {
    User {
      uid: profile.uid,
      uuid: profile.uuid.to_string(),
      email: profile.email,
      name: profile.name,
      latest_workspace_id: profile.latest_workspace_id.to_string(),
    }
  }
}
