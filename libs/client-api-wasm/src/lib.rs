pub mod entities;
use crate::entities::{ClientAPIConfig, ClientResponse};
use client_api::{Client, ClientConfiguration};
use tracing;
use wasm_bindgen::prelude::*;

// When the `wee_alloc` feature is enabled, use `wee_alloc` as the global
// allocator.
#[cfg(feature = "wee_alloc")]
#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

#[wasm_bindgen]
extern "C" {
  #[wasm_bindgen(js_namespace = console)]
  fn log(msg: &str);

  #[wasm_bindgen(js_namespace = console)]
  fn error(msg: &str);

  #[wasm_bindgen(js_namespace = console)]
  fn info(msg: &str);

  #[wasm_bindgen(js_namespace = console)]
  fn debug(msg: &str);

  #[wasm_bindgen(js_namespace = console)]
  fn warn(msg: &str);

  #[wasm_bindgen(js_namespace = console)]
  fn trace(msg: &str);

}

#[wasm_bindgen]
pub struct ClientAPI {
  client: Client,
}

#[wasm_bindgen]
impl ClientAPI {
  pub fn new(config: ClientAPIConfig) -> ClientAPI {
    tracing_wasm::set_as_global_default();
    let configuration = ClientConfiguration::default();

    if let Some(compression) = &config.configuration {
      configuration
        .to_owned()
        .with_compression_buffer_size(compression.compression_buffer_size)
        .with_compression_quality(compression.compression_quality);
    }

    let client = Client::new(
      config.base_url.as_str(),
      config.ws_addr.as_str(),
      config.gotrue_url.as_str(),
      config.device_id.as_str(),
      configuration,
      config.client_id.as_str(),
    );
    tracing::debug!("Client API initialized, config: {:?}", config);
    ClientAPI { client }
  }

  // pub async fn get_user(&self) -> ClientResponse {
  // 	if let Err(err) = self.client.get_profile().await {
  // 		log::error!("Get user failed: {:?}", err);
  // 		return ClientResponse<bool> {
  // 			code: ClientErrorCode::from(err.code),
  // 			message: err.message.to_string(),
  // 			data: None
  // 		}
  // 	}
  //
  // 	log::info!("Get user success");
  // 	ClientResponse {
  // 		code: ClientErrorCode::Ok,
  // 		message: "Get user success".to_string(),
  // 	}
  // }

  pub async fn sign_up_email_verified(
    &self,
    email: &str,
    password: &str,
  ) -> Result<bool, ClientResponse> {
    if let Err(err) = self.client.sign_up(email, password).await {
      return Err(ClientResponse {
        code: err.code,
        message: err.message.to_string(),
      });
    }

    Ok(true)
  }

  pub async fn sign_in_password(
    &self,
    email: &str,
    password: &str,
  ) -> Result<bool, ClientResponse> {
    if let Err(err) = self.client.sign_in_password(email, password).await {
      return Err(ClientResponse {
        code: err.code,
        message: err.message.to_string(),
      });
    }

    Ok(true)
  }
}
