use anyhow::{Error, Ok};

use crate::{
  component::auth::gotrue::models::{AccessTokenResponse, OAuthError, TokenResult, User},
  utils::http_response::{check_response, from_response},
};

const HEADER_TOKEN: &str = "token";

pub struct Client {
  http_client: reqwest::Client,
  base_url: String,
  token: Option<String>,
}

impl Client {
  pub fn from(c: reqwest::Client, base_url: &str) -> Self {
    Self {
      base_url: base_url.to_string(),
      http_client: c,
      token: None,
    }
  }

  pub async fn sign_in_password(&self, email: &str, password: &str) -> Result<TokenResult, Error> {
    let url = format!("{}/api/user/sign_in/password", self.base_url);
    let payload = serde_json::json!({
        "email": email,
        "password": password,
    });
    let resp = self.http_client.post(&url).json(&payload).send().await?;
    Ok(from_response(resp).await?)
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
    self.token.as_deref()
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
    self.token = Some(token.token);
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
    self.token = Some(token.token);
    Ok(())
  }

  pub async fn change_password(
    &self,
    current_password: &str,
    new_password: &str,
    new_password_confirm: &str,
  ) -> Result<(), Error> {
    let auth_token = match &self.token {
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
