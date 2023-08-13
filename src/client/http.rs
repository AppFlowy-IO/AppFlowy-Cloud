use anyhow::Error;

use crate::{
  component::auth::gotrue::models::{AccessTokenResponse, OAuthError, TokenResult, User},
  utils::http_response::{check_response, from_response},
};

const HEADER_TOKEN: &str = "token";

pub struct Client {
  http_client: reqwest::Client,
  base_url: String,
  token: Option<AccessTokenResponse>,
  token_old: Option<String>,
}

impl Client {
  pub fn from(c: reqwest::Client, base_url: &str) -> Self {
    Self {
      base_url: base_url.to_string(),
      http_client: c,
      token_old: None,
      token: None,
    }
  }

  pub fn token(&self) -> Option<&AccessTokenResponse> {
    self.token.as_ref()
  }

  pub async fn sign_in_password(
    &mut self,
    email: &str,
    password: &str,
  ) -> Result<Result<(), OAuthError>, Error> {
    let url = format!("{}/api/user/sign_in/password", self.base_url);
    let payload = serde_json::json!({
        "email": email,
        "password": password,
    });
    let resp = self.http_client.post(&url).json(&payload).send().await?;
    let token_result: TokenResult = from_response(resp).await?;
    match token_result {
      TokenResult::Success(s) => {
        self.token = Some(s);
        Ok(Ok(()))
      },
      TokenResult::Fail(e) => Ok(Err(e)),
    }
  }

  pub async fn sign_up(&self, email: &str, password: &str) -> Result<User, Error> {
    let url = format!("{}/api/user/sign_up", self.base_url);
    let payload = serde_json::json!({
        "email": email,
        "password": password,
    });
    let resp = self.http_client.post(&url).json(&payload).send().await?;
    Ok(from_response(resp).await?)
  }

  // returns logged_in token if logged_in
  pub fn logged_in_token(&self) -> Option<&str> {
    self.token_old.as_deref()
  }

  pub async fn register(&mut self, name: &str, email: &str, password: &str) -> Result<(), Error> {
    let url = format!("{}/api/user/register", self.base_url);
    let payload = serde_json::json!({
        "name": name,
        "password": password,
        "email": email,
    });
    let resp = self.http_client.post(&url).json(&payload).send().await?;
    let token: Token = from_response(resp).await?;
    self.token_old = Some(token.token);
    Ok(())
  }

  pub async fn login(&mut self, email: &str, password: &str) -> Result<(), Error> {
    let url = format!("{}/api/user/login", self.base_url);
    let payload = serde_json::json!({
        "password": password,
        "email": email,
    });
    let resp = self.http_client.post(&url).json(&payload).send().await?;
    let token: Token = from_response(resp).await?;
    self.token_old = Some(token.token);
    Ok(())
  }

  pub async fn change_password(
    &self,
    current_password: &str,
    new_password: &str,
    new_password_confirm: &str,
  ) -> Result<(), Error> {
    let auth_token = match &self.token_old {
      Some(t) => t.to_string(),
      None => anyhow::bail!("no token found, are you logged in?"),
    };

    let url = format!("{}/api/user/password", self.base_url);
    let payload = serde_json::json!({
        "current_password": current_password,
        "new_password": new_password,
        "new_password_confirm": new_password_confirm,
    });
    let resp = self
      .http_client
      .post(&url)
      .header(HEADER_TOKEN, auth_token)
      .json(&payload)
      .send()
      .await?;
    check_response(resp).await
  }
}

// Models
#[derive(serde::Deserialize)]
struct Token {
  token: String,
}
