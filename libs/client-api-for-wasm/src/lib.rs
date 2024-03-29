mod entities;
mod logger;

use crate::entities::{ClientAPIConfig, ClientErrorCode, ClientResponse};
use crate::logger::init_logger;
use client_api::{Client, ClientConfiguration};
use wasm_bindgen::prelude::*;

// When the `wee_alloc` feature is enabled, use `wee_alloc` as the global
// allocator.
#[cfg(feature = "wee_alloc")]
#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

#[wasm_bindgen]
extern "C" {
  #[wasm_bindgen(js_namespace = window)]
  fn wasm_trace(level: &str, target: &str, msg: &str);
}

#[wasm_bindgen]
pub struct ClientAPI {
  client: Client,
}

#[wasm_bindgen]
impl ClientAPI {
<<<<<<< HEAD
	pub fn new(config: ClientAPIConfig) -> ClientAPI {
		init_logger();
		let configuration = ClientConfiguration::new(config.configuration.compression_quality, config.configuration.compression_buffer_size);
		let client = Client::new(config.base_url.as_str(), config.ws_addr.as_str(), config.gotrue_url.as_str(), config.device_id.as_str(), configuration, config.client_id.as_str());
		log::debug!("Client API initialized, config: {:?}", config);
		ClientAPI {
			client,
		}
	}
=======
  pub fn new(config: ClientAPIConfig) -> ClientAPI {
    init_logger();
    let configuration = ClientConfiguration::default();
    configuration
      .to_owned()
      .with_compression_buffer_size(config.configuration.compression_buffer_size);
    configuration
      .to_owned()
      .with_compression_quality(config.configuration.compression_quality);
>>>>>>> eb1aa08 (fix: cargo fmt)

    let client = Client::new(
      config.base_url.as_str(),
      config.ws_addr.as_str(),
      config.gotrue_url.as_str(),
      config.device_id.as_str(),
      configuration,
      config.client_id.as_str(),
    );
    log::debug!("Client API initialized, config: {:?}", config);
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

  pub async fn sign_in_password(
    &self,
    email: &str,
    password: &str,
  ) -> Result<bool, ClientResponse> {
    if let Err(err) = self.client.sign_in_password(email, password).await {
      log::error!("Sign in failed: {:?}", err);
      return Err(ClientResponse {
        code: ClientErrorCode::from(err.code),
        message: err.message.to_string(),
      });
    }

    log::info!("Sign in success");
    Ok(true)
  }
}
