use serde::{Deserialize, Serialize};
use tsify::Tsify;
use wasm_bindgen::JsValue;
use client_api::error::{ErrorCode};

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
	pub configuration: Configuration,
	pub client_id: String,
}

#[derive(Tsify, Serialize, Deserialize, Default, Debug)]
#[repr(i32)]
pub enum ClientErrorCode {
	#[default]
	Ok = 0,
	Unhandled = -1,
	RecordNotFound = -2,
	RecordAlreadyExists = -3,
	InvalidEmail = 1001,
	InvalidPassword = 1002,
	OAuthError = 1003,
	MissingPayload = 1004,
	DBError = 1005,
	OpenError = 1006,
	InvalidUrl = 1007,
	InvalidRequest = 1008,
	InvalidOAuthProvider = 1009,
	NotLoggedIn = 1011,
	NotEnoughPermissions = 1012,
}

impl From<ErrorCode> for ClientErrorCode {
	fn from(value: ErrorCode) -> Self {
		match value {
			ErrorCode::Ok => Self::Ok,
			ErrorCode::Unhandled => Self::Unhandled,
			ErrorCode::RecordNotFound => Self::RecordNotFound,
			ErrorCode::RecordAlreadyExists => Self::RecordAlreadyExists,
			ErrorCode::InvalidEmail => Self::InvalidEmail,
			ErrorCode::InvalidPassword => Self::InvalidPassword,
			ErrorCode::OAuthError => Self::OAuthError,
			ErrorCode::MissingPayload => Self::MissingPayload,
			ErrorCode::DBError => Self::DBError,
			ErrorCode::OpenError => Self::OpenError,
			ErrorCode::InvalidUrl => Self::InvalidUrl,
			ErrorCode::InvalidRequest => Self::InvalidRequest,
			ErrorCode::InvalidOAuthProvider => Self::InvalidOAuthProvider,
			ErrorCode::NotLoggedIn => Self::NotLoggedIn,
			ErrorCode::NotEnoughPermissions => Self::NotEnoughPermissions,
			_ => Self::Unhandled,
		}
	}
}

#[derive(Tsify, Serialize, Deserialize, Default, Debug)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct ClientResponse {
	pub code: ClientErrorCode,
	pub message: String,
}

impl From<ClientResponse> for JsValue {
	fn from(value: ClientResponse) -> Self {
		JsValue::from_str(&serde_json::to_string(&value).unwrap())
	}
}