pub mod entities;

use crate::entities::*;

use client_api::notify::TokenState;
use client_api::{Client, ClientConfiguration};
use std::sync::Arc;

use wasm_bindgen::prelude::*;

#[cfg(feature = "enable_wee_alloc")]
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

  #[wasm_bindgen(js_namespace = window)]
  fn refresh_token(token: &str);

  #[wasm_bindgen(js_namespace = window)]
  fn invalid_token();
}

#[wasm_bindgen]
pub struct ClientAPI {
  client: Arc<Client>,
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
    ClientAPI {
      client: Arc::new(client),
    }
  }

  pub fn subscribe(&self) {
    let mut rx = self.client.subscribe_token_state();
    let client = self.client.clone();

    wasm_bindgen_futures::spawn_local(async move {
      while let Ok(state) = rx.recv().await {
        match state {
          TokenState::Refresh => {
            if let Ok(token) = client.get_token() {
              refresh_token(token.as_str());
            } else {
              invalid_token();
            }
          },
          TokenState::Invalid => {
            invalid_token();
          },
        }
      }
    });
  }
  pub async fn login(&self, email: &str, password: &str) -> Result<(), ClientResponse> {
    match self.client.sign_in_password(email, password).await {
      Ok(_) => Ok(()),
      Err(err) => Err(ClientResponse::from(err)),
    }
  }

  pub async fn sign_up(&self, email: &str, password: &str) -> Result<(), ClientResponse> {
    match self.client.sign_up(email, password).await {
      Ok(_) => Ok(()),
      Err(err) => Err(ClientResponse::from(err)),
    }
  }

  pub async fn logout(&self) -> Result<(), ClientResponse> {
    match self.client.sign_out().await {
      Ok(_) => Ok(()),
      Err(err) => Err(ClientResponse::from(err)),
    }
  }

  pub async fn get_user(&self) -> Result<User, ClientResponse> {
    match self.client.get_profile().await {
      Ok(profile) => Ok(User::from(profile)),
      Err(err) => Err(ClientResponse::from(err)),
    }
  }

  pub fn restore_token(&self, token: &str) -> Result<(), ClientResponse> {
    match self.client.restore_token(token) {
      Ok(_) => Ok(()),
      Err(err) => Err(ClientResponse::from(err)),
    }
  }

  pub async fn get_collab(
    &self,
    params: ClientQueryCollabParams,
  ) -> Result<ClientEncodeCollab, ClientResponse> {
    tracing::debug!("get_collab: {:?}", params);
    match self.client.get_collab(params.into()).await {
      Ok(data) => Ok(ClientEncodeCollab::from(data.encode_collab)),
      Err(err) => Err(ClientResponse::from(err)),
    }
  }
}
