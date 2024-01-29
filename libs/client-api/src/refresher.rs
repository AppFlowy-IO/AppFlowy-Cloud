use std::sync::Arc;

use gotrue::grant::{Grant, RefreshTokenGrant};
use parking_lot::RwLock;
use reqwest::{Request, Response, StatusCode};
use reqwest_middleware::Result;
use reqwest_middleware::{Middleware, Next};
use task_local_extensions::Extensions;
use tracing::warn;

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
    let mut cloned_req = match req.try_clone() {
      Some(cloned_req) => cloned_req,
      None => return next.run(req, extensions).await,
    };

    let next_cloned = next.clone();
    let res = match next.run(req, extensions).await {
      Ok(res) => res,
      Err(err) => return Err(err),
    };

    match res.status() {
      StatusCode::FORBIDDEN | StatusCode::BAD_REQUEST | StatusCode::UNAUTHORIZED => {},
      _ => return Ok(res),
    }

    let token = self.token.read().clone();

    let refresh_token = match token.as_ref() {
      Some(token) => token.refresh_token.as_str().to_owned(),
      None => return Ok(res),
    };

    warn!("Bad request or unauthorized, trying to refresh token");
    let gotrue_client = self.gotrue_client.clone();
    let grant = Grant::RefreshToken(RefreshTokenGrant { refresh_token });

    let new_token = match gotrue_client.token(&grant).await {
      Ok(new_token) => new_token,
      Err(err) => {
        tracing::error!("Error refreshing token: {:?}", err);
        return Ok(res);
      },
    };

    // clone access_token for next request
    let access_token = new_token.access_token.clone();

    // update token for next time use
    self.token.write().set(new_token);

    // update Authorization header
    let headers = cloned_req.headers_mut();
    match headers.remove("Authorization") {
      Some(_) => {},
      None => return Ok(res), // previous request doesn't have Authorization header
    };
    headers.insert(
      "Authorization",
      format!("Bearer {}", access_token).parse().unwrap(),
    );

    // Do request for the second time with new token
    match next_cloned.run(cloned_req, extensions).await {
      Ok(new_res) => Ok(new_res),
      Err(err) => {
        tracing::error!("successful refresh but still got error: {:?}", err);
        Ok(res)
      },
    }
  }
}
