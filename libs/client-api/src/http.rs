use anyhow::Error;
use reqwest::Method;
use reqwest::RequestBuilder;
use shared_entity::data::AppResponse;

use gotrue::models::{AccessTokenResponse, User};
use infra::reqwest::from_response;

use shared_entity::server_error::ErrorCode;

pub struct Client {
  http_client: reqwest::Client,
  base_url: String,
  token: Option<AccessTokenResponse>,
}

impl Client {
  pub fn from(c: reqwest::Client, base_url: &str) -> Self {
    Self {
      base_url: base_url.to_string(),
      http_client: c,
      token: None,
    }
  }

  pub fn token(&self) -> Option<&AccessTokenResponse> {
    self.token.as_ref()
  }

  pub async fn sign_in_password(&mut self, email: &str, password: &str) -> Result<(), Error> {
    let url = format!("{}/api/user/sign_in/password", self.base_url);
    let payload = serde_json::json!({
        "email": email,
        "password": password,
    });
    let resp = self.http_client.post(&url).json(&payload).send().await?;
    let token: AppResponse<AccessTokenResponse> = from_response(resp).await?;
    self.token = token.into_inner()?;
    Ok(())
  }

  pub async fn sign_up(&self, email: &str, password: &str) -> Result<(), Error> {
    let url = format!("{}/api/user/sign_up", self.base_url);
    let payload = serde_json::json!({
        "email": email,
        "password": password,
    });
    let resp = self.http_client.post(&url).json(&payload).send().await?;
    let res: AppResponse<()> = from_response(resp).await?;
    match res.into_error() {
      None => Ok(()),
      Some(e) => Err(e.into()),
    }
  }

  pub async fn sign_out(&self) -> Result<AppResponse<()>, Error> {
    let url = format!("{}/api/user/sign_out", self.base_url);
    let resp = self
      .http_client_with_auth(Method::POST, &url)?
      .send()
      .await?;
    let res: AppResponse<()> = from_response(resp).await?;
    Ok(res)
  }

  pub async fn update(&mut self, email: &str, password: &str) -> Result<(), Error> {
    let url = format!("{}/api/user/update", self.base_url);
    let payload = serde_json::json!({
        "email": email,
        "password": password,
    });
    let resp = self
      .http_client_with_auth(Method::POST, &url)?
      .json(&payload)
      .send()
      .await?;
    let new_user: AppResponse<User> = from_response(resp).await?;
    let new_user = new_user
      .into_inner()?
      .ok_or::<Error>(ErrorCode::Unhandled.into())?;
    if let Some(t) = self.token.as_mut() {
      t.user = new_user;
    }
    Ok(())
  }

  fn http_client_with_auth(&self, method: Method, url: &str) -> Result<RequestBuilder, Error> {
    match &self.token {
      None => anyhow::bail!("no token found, are you logged in?"),
      Some(t) => Ok(
        self
          .http_client
          .request(method, url)
          .bearer_auth(t.access_token.to_string()),
      ),
    }
  }

  // pub async fn change_password(
  //   &self,
  //   current_password: &str,
  //   new_password: &str,
  //   new_password_confirm: &str,
  // ) -> Result<(), Error> {
  //   let auth_token = match &self.token_old {
  //     Some(t) => t.to_string(),
  //     None => anyhow::bail!("no token found, are you logged in?"),
  //   };

  //   let url = format!("{}/api/user/password", self.base_url);
  //   let payload = serde_json::json!({
  //       "current_password": current_password,
  //       "new_password": new_password,
  //       "new_password_confirm": new_password_confirm,
  //   });
  //   let resp = self
  //     .http_client
  //     .post(&url)
  //     .header(HEADER_TOKEN, auth_token)
  //     .json(&payload)
  //     .send()
  //     .await?;
  //   check_response(resp).await
  // }
}
