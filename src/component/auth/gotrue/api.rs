use anyhow::Error;
use futures_util::TryFutureExt;

use super::{
  grant::Grant,
  models::{AccessTokenResponse, GoTrueError, GoTrueSettings, OAuthError, TokenResult, User},
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

  pub async fn settings(&self) -> Result<GoTrueSettings, Error> {
    let url: String = format!("{}/settings", self.base_url);
    let resp = self.client.get(url).send().await?;
    from_response(resp).await
  }

  pub async fn sign_up(&self, email: &str, password: &str) -> Result<User, Error> {
    let payload = serde_json::json!({
        "email": email,
        "password": password,
    });
    let url: String = format!("{}/signup", self.base_url);

    let (settings, resp) = tokio::try_join!(
      self.settings(),
      self
        .client
        .post(&url)
        .json(&payload)
        .send()
        .map_err(Error::from),
    )?;

    if settings.mailer_autoconfirm {
      let token: AccessTokenResponse = from_response(resp).await?;
      Ok(token.user)
    } else {
      from_response(resp).await
    }
  }

  pub async fn token(&self, grant: &Grant) -> Result<TokenResult, Error> {
    let url = format!("{}/token?grant_type={}", self.base_url, grant.type_as_str());
    let payload = grant.json_value();
    let resp = self.client.post(url).json(&payload).send().await?;
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
      .post(format!("{}/logout", self.base_url))
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
  ) -> Result<Result<User, GoTrueError>, Error> {
    let payload = serde_json::json!({
        "email": email,
        "password": password,
    });
    let resp = self
      .client
      .put(format!("{}/user", self.base_url))
      .header("Authorization", format!("Bearer {}", access_token))
      .json(&payload)
      .send()
      .await?;

    if resp.status().is_success() {
      let user: User = from_body(resp).await?;
      Ok(Ok(user))
    } else {
      let err: GoTrueError = from_body(resp).await?;
      Ok(Err(err))
    }
  }
}
