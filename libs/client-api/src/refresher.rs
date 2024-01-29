use std::sync::Arc;

use gotrue::grant::{Grant, RefreshTokenGrant};
use parking_lot::RwLock;
use reqwest::{Request, Response, StatusCode};
use reqwest_middleware::Result;
use reqwest_middleware::{Middleware, Next};
use task_local_extensions::Extensions;
use tracing::{info, warn};

use crate::notify::ClientToken;

pub struct Refresher {
  token: Arc<RwLock<ClientToken>>,
  gotrue_client: Arc<gotrue::api::Client>,
}

// Retry once with refresh token if token is expired
impl Refresher {
  pub fn new(token: Arc<RwLock<ClientToken>>, gotrue_url: &str) -> Self {
    Self {
      token,
      gotrue_client: Arc::new(gotrue::api::Client::new(
        reqwest::Client::new().into(),
        gotrue_url,
      )),
    }
  }
}

#[async_trait::async_trait]
impl Middleware for Refresher {
  async fn handle(
    &self,
    req: Request,
    extensions: &mut Extensions,
    next: Next<'_>,
  ) -> Result<Response> {
    match req.try_clone() {
      Some(mut cloned_req) => {
        let next_cloned = next.clone();
        let res = next.run(req, extensions).await;
        match res {
          Ok(res) => match res.status() {
            StatusCode::FORBIDDEN | StatusCode::BAD_REQUEST | StatusCode::UNAUTHORIZED => {
              warn!("Bad request or unauthorized, trying to refresh token");

              let token = self.token.read().clone();
              let refresh_token = token
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("No refresh token found in Refresher middleware"))?
                .refresh_token
                .as_str()
                .to_owned();

              let gotrue_client = self.gotrue_client.clone();
              let grant = Grant::RefreshToken(RefreshTokenGrant { refresh_token });

              match gotrue_client.token(&grant).await {
                Ok(new_token) => {
                  let access_token = new_token.access_token.clone();
                  // update token for next time use
                  self.token.write().set(new_token);

                  let headers = cloned_req.headers_mut();
                  match headers.remove("Authorization") {
                    Some(prev_auth) => {
                      info!("previouse auth: {:?}", prev_auth);
                      headers.insert(
                        "Authorization",
                        format!("Bearer {}", access_token).parse().unwrap(),
                      );
                      next_cloned.run(cloned_req, extensions).await
                    },
                    // No previous auth header, just return the response
                    None => Ok(res),
                  }
                },
                Err(err) => {
                  tracing::error!("Error refreshing token: {:?}", err);
                  Ok(res)
                },
              }
            },
            _ => Ok(res),
          },
          Err(_) => res,
        }
      },
      None => next.run(req, extensions).await,
    }
  }
}
