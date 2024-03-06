use super::grant::Grant;
use crate::params::{
  AdminDeleteUserParams, AdminUserParams, CreateSSOProviderParams, GenerateLinkParams,
  GenerateLinkResponse, InviteUserParams, MagicLinkParams,
};
use anyhow::Context;
use gotrue_entity::dto::{
  AdminListUsersResponse, AuthProvider, GoTrueSettings, GotrueTokenResponse, SignUpResponse,
  UpdateGotrueUserParams, User,
};
use gotrue_entity::error::{GoTrueError, GoTrueErrorSerde, GotrueClientError};
use gotrue_entity::sso::{SSOProvider, SSOProviders};
use infra::reqwest::{check_response, from_body, from_response};
use reqwest::{Method, RequestBuilder};
use tracing::event;

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

  pub fn oauth_url(&self, provider: &AuthProvider) -> String {
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
  pub async fn sign_up(
    &self,
    email: &str,
    password: &str,
    redirect_to: Option<&str>,
  ) -> Result<SignUpResponse, GoTrueError> {
    let payload = serde_json::json!({
        "email": email,
        "password": password,
    });
    let url: String = format!("{}/signup", self.base_url);
    let mut req_builder = self.client.post(&url).json(&payload);
    if let Some(redirect_to) = redirect_to {
      req_builder = req_builder.header("redirect_to", redirect_to);
    }

    let resp = req_builder.send().await?;
    to_gotrue_result(resp).await
  }

  #[tracing::instrument(skip_all, err)]
  pub async fn token(&self, grant: &Grant) -> Result<GotrueTokenResponse, GoTrueError> {
    // https://github.com/supabase/gotrue/blob/master/internal/api/verify.go#L219
    let url = format!("{}/token?grant_type={}", self.base_url, grant.type_as_str());
    let payload = grant.json_value();
    let resp = self.client.post(url).json(&payload).send().await?;
    if resp.status().is_success() {
      let token: GotrueTokenResponse = from_body(resp).await?;
      Ok(token)
    } else if resp.status().is_client_error() {
      Err(from_body::<GotrueClientError>(resp).await?.into())
    } else {
      Err(anyhow::anyhow!("unexpected response status: {}", resp.status()).into())
    }
  }

  #[tracing::instrument(skip_all, err)]
  pub async fn logout(&self, access_token: &str) -> Result<(), GoTrueError> {
    let url = format!("{}/logout", self.base_url);
    let resp = self
      .http_client_with_auth(Method::POST, &url, access_token)
      .send()
      .await?;
    Ok(check_response(resp).await?)
  }

  #[tracing::instrument(skip_all, err)]
  pub async fn user_info(&self, access_token: &str) -> Result<User, GoTrueError> {
    let url = format!("{}/user", self.base_url);
    match self
      .http_client_with_auth(Method::GET, &url, access_token)
      .send()
      .await
    {
      Ok(resp) => to_gotrue_result(resp).await,
      Err(err) => {
        event!(
          tracing::Level::ERROR,
          "fail to get user info with access token: {}",
          access_token
        );
        Err(err.into())
      },
    }
  }

  pub async fn update_user(
    &self,
    access_token: &str,
    update_user_params: &UpdateGotrueUserParams,
  ) -> Result<User, GoTrueError> {
    let url = format!("{}/user", self.base_url);
    let resp = self
      .http_client_with_auth(Method::PUT, &url, access_token)
      .json(update_user_params)
      .send()
      .await?;

    to_gotrue_result(resp).await
  }

  pub async fn admin_list_user(
    &self,
    access_token: &str,
    filter: Option<&str>,
  ) -> Result<AdminListUsersResponse, GoTrueError> {
    let url = format!("{}/admin/users", self.base_url);
    let mut req = self.http_client_with_auth(Method::GET, &url, access_token);
    if let Some(filter) = filter {
      req = req.query(&[("filter", filter)]);
    }
    let resp = req.send().await?;
    to_gotrue_result(resp).await
  }

  pub async fn admin_user_details(
    &self,
    access_token: &str,
    user_id: &str, // uuid
  ) -> Result<User, GoTrueError> {
    let url = format!("{}/admin/users/{}", self.base_url, user_id);
    let resp = self
      .http_client_with_auth(Method::GET, &url, access_token)
      .send()
      .await?;
    to_gotrue_result(resp).await
  }

  pub async fn admin_delete_user(
    &self,
    access_token: &str,
    user_uuid: &str,
    delete_user_params: &AdminDeleteUserParams,
  ) -> Result<(), GoTrueError> {
    let url = format!("{}/admin/users/{}", self.base_url, user_uuid);
    let resp = self
      .http_client_with_auth(Method::DELETE, &url, access_token)
      .json(&delete_user_params)
      .send()
      .await?;
    check_gotrue_result(resp).await
  }

  pub async fn admin_update_user(
    &self,
    access_token: &str,
    user_uuid: &str,
    admin_user_params: &AdminUserParams,
  ) -> Result<User, GoTrueError> {
    let url = format!("{}/admin/users/{}", self.base_url, user_uuid);
    let resp = self
      .http_client_with_auth(Method::PUT, &url, access_token)
      .json(&admin_user_params)
      .send()
      .await?;
    to_gotrue_result(resp).await
  }

  pub async fn admin_invite_user(
    &self,
    access_token: &str,
    admin_user_params: &InviteUserParams,
  ) -> Result<User, GoTrueError> {
    let url = format!("{}/invite", self.base_url);
    let resp = self
      .http_client_with_auth(Method::POST, &url, access_token)
      .json(&admin_user_params)
      .send()
      .await?;
    to_gotrue_result(resp).await
  }

  pub async fn admin_add_user(
    &self,
    access_token: &str,
    admin_user_params: &AdminUserParams,
  ) -> Result<User, GoTrueError> {
    let url = format!("{}/admin/users", self.base_url);
    let resp = self
      .http_client_with_auth(Method::POST, &url, access_token)
      .json(&admin_user_params)
      .send()
      .await?;
    to_gotrue_result(resp).await
  }

  pub async fn admin_generate_link(
    &self,
    access_token: &str,
    generate_link_params: &GenerateLinkParams,
  ) -> Result<GenerateLinkResponse, GoTrueError> {
    let url = format!("{}/admin/generate_link", self.base_url);
    let resp = self
      .http_client_with_auth(Method::POST, &url, access_token)
      .json(&generate_link_params)
      .send()
      .await?;
    to_gotrue_result(resp).await
  }

  pub async fn magic_link(
    &self,
    magic_link_params: &MagicLinkParams,
    redirect_to: Option<String>,
  ) -> Result<(), GoTrueError> {
    let url = format!("{}/magiclink", self.base_url);
    let mut req_builder = self.client.request(Method::POST, &url);
    if let Some(redirect_to) = redirect_to {
      req_builder = req_builder.header("redirect_to", redirect_to);
    }
    let resp = req_builder.json(&magic_link_params).send().await?;
    check_gotrue_result(resp).await
  }

  pub async fn admin_list_sso_providers(
    &self,
    access_token: &str,
  ) -> Result<SSOProviders, GoTrueError> {
    let url = format!("{}/admin/sso/providers", self.base_url);
    let resp = self
      .http_client_with_auth(Method::GET, &url, access_token)
      .send()
      .await?;
    to_gotrue_result(resp).await
  }

  pub async fn admin_create_sso_providers(
    &self,
    access_token: &str,
    create_sso_provider_params: &CreateSSOProviderParams,
  ) -> Result<SSOProvider, GoTrueError> {
    let url = format!("{}/admin/sso/providers", self.base_url);
    let resp = self
      .http_client_with_auth(Method::POST, &url, access_token)
      .json(create_sso_provider_params)
      .send()
      .await?;
    to_gotrue_result(resp).await
  }

  pub async fn admin_get_sso_provider(
    &self,
    access_token: &str,
    idp_id: &str,
  ) -> Result<SSOProvider, GoTrueError> {
    let url = format!("{}/admin/sso/providers/{}", self.base_url, idp_id);
    let resp = self
      .http_client_with_auth(Method::GET, &url, access_token)
      .send()
      .await?;
    to_gotrue_result(resp).await
  }

  pub async fn admin_update_sso_provider(
    &self,
    access_token: &str,
    idp_id: &str,
    create_sso_provider_params: &CreateSSOProviderParams,
  ) -> Result<SSOProvider, GoTrueError> {
    let url = format!("{}/admin/sso/providers/{}", self.base_url, idp_id);
    let resp = self
      .http_client_with_auth(Method::PUT, &url, access_token)
      .json(create_sso_provider_params)
      .send()
      .await?;
    to_gotrue_result(resp).await
  }

  pub async fn admin_delete_sso_provider(
    &self,
    access_token: &str,
    idp_id: &str,
  ) -> Result<SSOProvider, GoTrueError> {
    let url = format!("{}/admin/sso/providers/{}", self.base_url, idp_id);
    let resp = self
      .http_client_with_auth(Method::DELETE, &url, access_token)
      .send()
      .await?;
    to_gotrue_result(resp).await
  }

  pub fn http_client_with_auth(
    &self,
    method: Method,
    url: &str,
    access_token: &str,
  ) -> RequestBuilder {
    self.client.request(method, url).bearer_auth(access_token)
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
    let err: GoTrueErrorSerde = from_body(resp).await?;
    Err(GoTrueError::Internal(err))
  }
}

async fn check_gotrue_result(resp: reqwest::Response) -> Result<(), GoTrueError> {
  if resp.status().is_success() {
    Ok(())
  } else {
    let err: GoTrueErrorSerde = from_body(resp).await?;
    Err(GoTrueError::Internal(err))
  }
}
