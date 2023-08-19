use anyhow::Error;

use super::{
  grant::Grant,
  models::{AccessTokenResponse, OAuthError, TokenResult, User},
};
use crate::utils::http_response::{check_response, from_body, from_response};

pub struct Client {
  client: reqwest::Client,
  base_url: String,
}

impl Client {
  pub fn new(client: reqwest::Client, url: &str) -> Self {
    Self {
      client,
      base_url: url.to_string(),
    }
  }

  pub async fn sign_up(&self, email: &str, password: &str) -> Result<User, Error> {
    let payload = serde_json::json!({
        "email": email,
        "password": password,
    });
    let url: String = format!("{}/signup", self.base_url);
    let resp = self.client.post(url.as_str()).json(&payload).send().await?;
    from_response(resp).await
  }

  pub async fn token(&self, grant: &Grant) -> Result<TokenResult, Error> {
    let url = format!(
      "{}/token?grant_type={}",
      self.base_url.as_str(),
      grant.type_as_str()
    );
    let payload = grant.json_value();
    let resp = self.client.post(url.as_str()).json(&payload).send().await?;
    if resp.status().is_success() {
      let token: AccessTokenResponse = from_body(resp).await?;
      Ok(TokenResult::Success(token))
    } else if resp.status().is_client_error() {
      let err: OAuthError = from_body(resp).await?;
      Ok(TokenResult::Fail(err))
    } else {
      anyhow::bail!("unexpected response status: {}", resp.status());
    }
  }

  pub async fn logout(&self, access_token: &str) -> Result<(), Error> {
    let resp = self
      .client
      .post(format!("{}/logout", self.base_url.as_str()).as_str())
      .header("Authorization", format!("Bearer {}", access_token))
      .send()
      .await?;
    check_response(resp).await
  }

  pub async fn update_user(
    &self,
    access_token: &str,
    email: &str,
    password: &str,
    phone: &str,
  ) -> Result<User, Error> {
    let payload = serde_json::json!({
        "email": email,
        "password": password,
        "phone": phone,

    });
    let resp = self
      .client
      .put(format!("{}/user", self.base_url.as_str()).as_str())
      .header("Authorization", format!("Bearer {}", access_token))
      .json(&payload)
      .send()
      .await?;
    from_response(resp).await
  }
}
