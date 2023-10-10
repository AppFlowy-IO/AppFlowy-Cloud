use crate::params::{AdminUserParams, GenerateLinkParams, GenerateLinkResponse};
use anyhow::Context;

use super::grant::Grant;
use gotrue_entity::{
  AccessTokenResponse, AdminListUsersResponse, GoTrueError, GoTrueSettings, OAuthError,
  OAuthProvider, SignUpResponse, User,
};
use infra::reqwest::{check_response, from_body, from_response};

#[derive(Clone)]
pub struct Client {
  client: reqwest::Client,
  pub base_url: String,
}

impl Client {
  pub fn new(client: reqwest::Client, base_url: &str) -> Self {
    Self {
      client,
      base_url: base_url.to_owned(),
    }
  }

  pub fn oauth_url(&self, provider: &OAuthProvider) -> String {
    format!("{}/authorize?provider={}", self.base_url, provider.as_str())
  }

  #[tracing::instrument(skip_all, err)]
  pub async fn health(&self) -> Result<(), GoTrueError> {
    let url: String = format!("{}/health", self.base_url);
    let resp = self
      .client
      .get(&url)
      .send()
      .await
      .context(format!("calling {} failed", url))?;
    Ok(check_response(resp).await?)
  }

  #[tracing::instrument(skip_all, err)]
  pub async fn settings(&self) -> Result<GoTrueSettings, GoTrueError> {
    let url: String = format!("{}/settings", self.base_url);
    let resp = self.client.get(&url).send().await?;
    let settings: GoTrueSettings = from_response(resp)
      .await
      .context(format!("calling {} failed", url))?;
    Ok(settings)
  }

  #[tracing::instrument(skip_all, err)]
  pub async fn sign_up(&self, email: &str, password: &str) -> Result<SignUpResponse, GoTrueError> {
    let payload = serde_json::json!({
        "email": email,
        "password": password,
    });
    let url: String = format!("{}/signup", self.base_url);
    let resp = self.client.post(&url).json(&payload).send().await?;
    to_gotrue_result(resp).await
  }

  #[tracing::instrument(skip_all, err)]
  pub async fn token(&self, grant: &Grant) -> Result<AccessTokenResponse, GoTrueError> {
    let url = format!("{}/token?grant_type={}", self.base_url, grant.type_as_str());
    let payload = grant.json_value();
    let resp = self.client.post(url).json(&payload).send().await?;
    if resp.status().is_success() {
      let token: AccessTokenResponse = from_body(resp).await?;
      Ok(token)
    } else if resp.status().is_client_error() {
      Err(from_body::<OAuthError>(resp).await?.into())
    } else {
      Err(anyhow::anyhow!("unexpected response status: {}", resp.status()).into())
    }
  }

  #[tracing::instrument(skip_all, err)]
  pub async fn logout(&self, access_token: &str) -> Result<(), GoTrueError> {
    let resp = self
      .client
      .post(format!("{}/logout", self.base_url))
      .header("Authorization", format!("Bearer {}", access_token))
      .send()
      .await?;
    Ok(check_response(resp).await?)
  }

  #[tracing::instrument(skip_all, err)]
  pub async fn user_info(&self, access_token: &str) -> Result<User, GoTrueError> {
    let url = format!("{}/user", self.base_url);
    let resp = self
      .client
      .get(&url)
      .header("Authorization", format!("Bearer {}", access_token))
      .send()
      .await?;
    to_gotrue_result(resp).await
  }

  pub async fn update_user(
    &self,
    access_token: &str,
    email: Option<String>,
    password: Option<String>,
  ) -> Result<User, GoTrueError> {
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

    to_gotrue_result(resp).await
  }

  pub async fn admin_list_user(
    &self,
    access_token: &str,
  ) -> Result<AdminListUsersResponse, GoTrueError> {
    let resp = self
      .client
      .get(format!("{}/admin/users", self.base_url))
      .header("Authorization", format!("Bearer {}", access_token))
      .send()
      .await?;
    to_gotrue_result(resp).await
  }

  pub async fn admin_add_user(
    &self,
    access_token: &str,
    admin_user_params: &AdminUserParams,
  ) -> Result<User, GoTrueError> {
    let resp = self
      .client
      .post(format!("{}/admin/users", self.base_url))
      .header("Authorization", format!("Bearer {}", access_token))
      .json(&admin_user_params)
      .send()
      .await?;
    to_gotrue_result(resp).await
  }

  pub async fn generate_link(
    &self,
    access_token: &str,
    generate_link_params: &GenerateLinkParams,
  ) -> Result<GenerateLinkResponse, GoTrueError> {
    let resp = self
      .client
      .post(format!("{}/admin/generate_link", self.base_url))
      .header("Authorization", format!("Bearer {}", access_token))
      .json(&generate_link_params)
      .send()
      .await?;
    to_gotrue_result(resp).await
  }
}

async fn to_gotrue_result<T>(resp: reqwest::Response) -> Result<T, GoTrueError>
where
  T: serde::de::DeserializeOwned,
{
  if resp.status().is_success() {
    let t: T = from_body(resp).await?;
    Ok(t)
  } else {
    let err: GoTrueError = from_body(resp).await?;
    Err(err)
  }
}
