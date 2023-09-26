use anyhow::anyhow;
use gotrue::grant::Grant;
use gotrue::grant::PasswordGrant;
use gotrue::grant::RefreshTokenGrant;
use gotrue::params::{AdminUserParams, GenerateLinkParams};
use gotrue_entity::OAuthProvider;
use gotrue_entity::SignUpResponse::{Authenticated, NotAuthenticated};
use parking_lot::RwLock;
use reqwest::Method;
use reqwest::RequestBuilder;
use scraper::{Html, Selector};
use shared_entity::data::AppResponse;
use shared_entity::dto::SignInTokenResponse;
use shared_entity::dto::UpdateUsernameParams;
use shared_entity::dto::UserUpdateParams;
use shared_entity::dto::WorkspaceMembersParams;
use std::sync::Arc;
use std::time::SystemTime;
use storage_entity::AFWorkspaceMember;

use gotrue_entity::{AccessTokenResponse, User};

use crate::notify::{ClientToken, TokenStateReceiver};
use shared_entity::error::AppError;
use shared_entity::error_code::url_missing_param;
use shared_entity::error_code::ErrorCode;
use storage_entity::{AFUserProfileView, InsertCollabParams};
use storage_entity::{AFWorkspaces, QueryCollabParams};
use storage_entity::{DeleteCollabParams, RawData};

pub struct Client {
  pub(crate) cloud_client: reqwest::Client,
  pub(crate) gotrue_client: gotrue::api::Client,
  base_url: String,
  ws_addr: String,
  token: Arc<RwLock<ClientToken>>,
}

impl Client {
  pub fn from(c: reqwest::Client, base_url: &str, ws_addr: &str, gotrue_url: &str) -> Self {
    Self {
      base_url: base_url.to_string(),
      ws_addr: ws_addr.to_string(),
      cloud_client: c.clone(),
      gotrue_client: gotrue::api::Client::new(c, gotrue_url),
      token: Arc::new(RwLock::new(ClientToken::new())),
    }
  }

  pub fn subscribe_token_state(&self) -> TokenStateReceiver {
    self.token.read().subscribe()
  }

  // e.g. appflowy-flutter://#access_token=...&expires_in=3600&provider_token=...&refresh_token=...&token_type=bearer
  pub async fn sign_in_url(&self, url: &str) -> Result<bool, AppError> {
    let mut access_token: Option<String> = None;
    let mut token_type: Option<String> = None;
    let mut expires_in: Option<i64> = None;
    let mut expires_at: Option<i64> = None;
    let mut refresh_token: Option<String> = None;
    let mut provider_access_token: Option<String> = None;
    let mut provider_refresh_token: Option<String> = None;

    url::Url::parse(url)?
      .fragment()
      .ok_or(url_missing_param("fragment"))?
      .split('&')
      .try_for_each(|f| -> Result<(), AppError> {
        let (k, v) = f.split_once('=').ok_or(url_missing_param("key=value"))?;
        match k {
          "access_token" => {
            access_token = Some(v.to_string());
          },
          "token_type" => {
            token_type = Some(v.to_string());
          },
          "expires_in" => {
            expires_in = Some(v.parse::<i64>()?);
          },
          "expires_at" => {
            expires_at = Some(v.parse::<i64>()?);
          },
          "refresh_token" => {
            refresh_token = Some(v.to_string());
          },
          "provider_access_token" => {
            provider_access_token = Some(v.to_string());
          },
          "provider_refresh_token" => {
            provider_refresh_token = Some(v.to_string());
          },
          _ => {},
        };
        Ok(())
      })?;

    let access_token = access_token.ok_or(url_missing_param("access_token"))?;
    let (user, new) = self.verify_token(&access_token).await?;

    self.token.write().set(AccessTokenResponse {
      access_token,
      token_type: token_type.ok_or(url_missing_param("token_type"))?,
      expires_in: expires_in.ok_or(url_missing_param("expires_in"))?,
      expires_at: expires_at.ok_or(url_missing_param("expires_at"))?,
      refresh_token: refresh_token.ok_or(url_missing_param("refresh_token"))?,
      user,
      provider_access_token,
      provider_refresh_token,
    });

    Ok(new)
  }

  async fn verify_token(&self, access_token: &str) -> Result<(User, bool), AppError> {
    let user = self.gotrue_client.user_info(access_token).await?;
    let is_new = self.verify_token_cloud(access_token).await?;
    Ok((user, is_new))
  }

  async fn verify_token_cloud(&self, access_token: &str) -> Result<bool, AppError> {
    let url = format!("{}/api/user/verify/{}", self.base_url, access_token);
    let resp = self.cloud_client.get(&url).send().await?;
    let sign_in_resp: SignInTokenResponse = AppResponse::from_response(resp).await?.into_data()?;
    Ok(sign_in_resp.is_new)
  }

  pub async fn create_magic_link(&self, email: &str, password: &str) -> Result<User, AppError> {
    let user = self
      .gotrue_client
      .admin_add_user(
        &self.access_token()?,
        &AdminUserParams {
          email: email.to_owned(),
          password: Some(password.to_owned()),
          email_confirm: true,
          ..Default::default()
        },
      )
      .await?;
    Ok(user)
  }

  pub async fn create_email_verified_user(
    &self,
    email: &str,
    password: &str,
  ) -> Result<User, AppError> {
    let user = self
      .gotrue_client
      .admin_add_user(
        &self.access_token()?,
        &AdminUserParams {
          email: email.to_owned(),
          password: Some(password.to_owned()),
          email_confirm: true,
          ..Default::default()
        },
      )
      .await?;
    Ok(user)
  }

  /// Only expose this method for testing
  #[cfg(debug_assertions)]
  pub fn token(&self) -> Arc<RwLock<ClientToken>> {
    self.token.clone()
  }

  pub fn token_expires_at(&self) -> Result<i64, AppError> {
    match &self.token.try_read() {
      None => Err(AppError::new(ErrorCode::Unhandled, "Failed to read token")),
      Some(token) => Ok(token.as_ref().ok_or(ErrorCode::NotLoggedIn)?.expires_at),
    }
  }

  pub fn access_token(&self) -> Result<String, AppError> {
    match &self.token.try_read() {
      None => Err(AppError::new(ErrorCode::Unhandled, "Failed to read token")),
      Some(token) => Ok(
        token
          .as_ref()
          .ok_or(ErrorCode::NotLoggedIn)?
          .access_token
          .clone(),
      ),
    }
  }

  pub async fn oauth_login(&self, provider: &OAuthProvider) -> Result<(), AppError> {
    let settings = self.gotrue_client.settings().await?;
    if !settings.external.has_provider(provider) {
      return Err(ErrorCode::InvalidOAuthProvider.into());
    }

    let oauth_url = format!(
      "{}/authorize?provider={}",
      self.gotrue_client.base_url,
      provider.as_str(),
    );
    opener::open(oauth_url)?;
    Ok(())
  }

  pub async fn profile(&self) -> Result<AFUserProfileView, AppError> {
    let url = format!("{}/api/user/profile", self.base_url);
    let resp = self
      .http_client_with_auth(Method::GET, &url)
      .await?
      .send()
      .await?;
    AppResponse::<AFUserProfileView>::from_response(resp)
      .await?
      .into_data()
  }

  pub async fn workspaces(&self) -> Result<AFWorkspaces, AppError> {
    let url = format!("{}/api/workspace/list", self.base_url);
    let resp = self
      .http_client_with_auth(Method::GET, &url)
      .await?
      .send()
      .await?;
    AppResponse::<AFWorkspaces>::from_response(resp)
      .await?
      .into_data()
  }

  pub async fn get_workspace_members(
    &self,
    workspace_uuid: uuid::Uuid,
  ) -> Result<Vec<AFWorkspaceMember>, AppError> {
    let url = format!(
      "{}/api/workspace/{}/member/list",
      self.base_url, workspace_uuid
    );
    let resp = self
      .http_client_with_auth(Method::GET, &url)
      .await?
      .send()
      .await?;
    AppResponse::<Vec<AFWorkspaceMember>>::from_response(resp)
      .await?
      .into_data()
  }

  pub async fn add_workspace_members(
    &self,
    workspace_uuid: uuid::Uuid,
    member_emails: Vec<String>,
  ) -> Result<(), AppError> {
    let url = format!("{}/api/workspace/member/add", self.base_url);
    let req = WorkspaceMembersParams {
      workspace_uuid,
      member_emails,
    };
    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .json(&req)
      .send()
      .await?;
    AppResponse::<()>::from_response(resp).await?.into_error()?;
    Ok(())
  }

  pub async fn remove_workspace_members(
    &self,
    workspace_uuid: uuid::Uuid,
    member_uids: Vec<String>,
  ) -> Result<(), AppError> {
    let url = format!("{}/api/workspace/member/remove", self.base_url);
    let req = WorkspaceMembersParams {
      workspace_uuid,
      member_emails: member_uids,
    };
    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .json(&req)
      .send()
      .await?;
    AppResponse::<()>::from_response(resp).await?.into_error()?;
    Ok(())
  }

  pub async fn sign_in_password(&self, email: &str, password: &str) -> Result<bool, AppError> {
    let access_token_resp = self
      .gotrue_client
      .token(&Grant::Password(PasswordGrant {
        email: email.to_owned(),
        password: password.to_owned(),
      }))
      .await?;
    let is_new = self
      .verify_token_cloud(&access_token_resp.access_token)
      .await?;
    self.token.write().set(access_token_resp);
    Ok(is_new)
  }

  pub async fn refresh(&self) -> Result<(), AppError> {
    let refresh_token = self
      .token
      .read()
      .as_ref()
      .ok_or::<AppError>(ErrorCode::NotLoggedIn.into())?
      .refresh_token
      .as_str()
      .to_owned();
    let access_token_resp = self
      .gotrue_client
      .token(&Grant::RefreshToken(RefreshTokenGrant { refresh_token }))
      .await?;
    self.token.write().set(access_token_resp);
    Ok(())
  }

  pub async fn sign_up(&self, email: &str, password: &str) -> Result<(), AppError> {
    match self.gotrue_client.sign_up(email, password).await? {
      Authenticated(access_token_resp) => {
        self.token.write().set(access_token_resp);
        Ok(())
      },
      NotAuthenticated(user) => {
        tracing::info!("sign_up but not authenticated: {}", user.email);
        Ok(())
      },
    }
  }

  pub async fn sign_out(&self) -> Result<(), AppError> {
    self.gotrue_client.logout(&self.access_token()?).await?;
    Ok(())
  }

  pub async fn update(&self, params: UserUpdateParams) -> Result<(), AppError> {
    let updated_user = self
      .gotrue_client
      .update_user(&self.access_token()?, params.email, params.password)
      .await?;
    if let Some(t) = self.token.write().as_mut() {
      t.user = updated_user;
    }
    if let Some(name) = params.name {
      self.update_user_name(&name).await?;
    }
    Ok(())
  }

  pub async fn update_user_name(&self, new_name: &str) -> Result<(), AppError> {
    let url = format!("{}/api/user/update", self.base_url);
    let params = UpdateUsernameParams {
      new_name: new_name.to_string(),
    };
    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .json(&params)
      .send()
      .await?;
    AppResponse::<()>::from_response(resp).await?.into_error()
  }

  pub async fn create_collab(&self, params: InsertCollabParams) -> Result<(), AppError> {
    let url = format!("{}/api/collab/", self.base_url);
    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .json(&params)
      .send()
      .await?;
    AppResponse::<()>::from_response(resp).await?.into_error()
  }

  pub async fn update_collab(&self, params: InsertCollabParams) -> Result<(), AppError> {
    let url = format!("{}/api/collab/", self.base_url);
    let resp = self
      .http_client_with_auth(Method::PUT, &url)
      .await?
      .json(&params)
      .send()
      .await?;
    AppResponse::<()>::from_response(resp).await?.into_error()
  }

  pub async fn get_collab(&self, params: QueryCollabParams) -> Result<RawData, AppError> {
    let url = format!("{}/api/collab/", self.base_url);
    let resp = self
      .http_client_with_auth(Method::GET, &url)
      .await?
      .json(&params)
      .send()
      .await?;
    AppResponse::<RawData>::from_response(resp)
      .await?
      .into_data()
  }

  pub async fn delete_collab(&self, params: DeleteCollabParams) -> Result<(), AppError> {
    let url = format!("{}/api/collab/", self.base_url);
    let resp = self
      .http_client_with_auth(Method::DELETE, &url)
      .await?
      .json(&params)
      .send()
      .await?;
    AppResponse::<()>::from_response(resp).await?.into_error()
  }

  pub fn ws_url(&self, device_id: &str) -> Result<String, AppError> {
    let access_token = self.access_token()?;
    Ok(format!("{}/{}/{}", self.ws_addr, access_token, device_id))
  }

  pub async fn generate_sign_in_callback_url(
    &self,
    admin_user_email: &str,
    admin_user_password: &str,
    user_email: &str,
  ) -> Result<String, AppError> {
    let admin_token = self
      .gotrue_client
      .token(&Grant::Password(PasswordGrant {
        email: admin_user_email.to_string(),
        password: admin_user_password.to_string(),
      }))
      .await?;

    let admin_user_params: GenerateLinkParams = GenerateLinkParams {
      email: user_email.to_string(),
      ..Default::default()
    };

    let link_resp = self
      .gotrue_client
      .generate_link(&admin_token.access_token, &admin_user_params)
      .await?;
    assert_eq!(link_resp.email, user_email);

    let action_link = link_resp.action_link;
    let resp = reqwest::Client::new().get(action_link).send().await?;
    let resp_text = resp.text().await?;
    Ok(extract_sign_in_url(&resp_text)?)
  }

  async fn http_client_with_auth(
    &self,
    method: Method,
    url: &str,
  ) -> Result<RequestBuilder, AppError> {
    let expires_at = self.token_expires_at()?;

    // Refresh token if it's about to expire
    let time_now_sec = SystemTime::now()
      .duration_since(SystemTime::UNIX_EPOCH)
      .unwrap()
      .as_secs() as i64;
    if time_now_sec + 60 > expires_at {
      // Add 60 seconds buffer
      self.refresh().await?;
    }

    let access_token = self.access_token()?;
    let request_builder = self
      .cloud_client
      .request(method, url)
      .bearer_auth(access_token);
    Ok(request_builder)
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

pub fn extract_sign_in_url(html_str: &str) -> Result<String, anyhow::Error> {
  let fragment = Html::parse_fragment(html_str);
  let selector = Selector::parse("a").unwrap();
  let url = fragment
    .select(&selector)
    .next()
    .ok_or(anyhow!("no a tag found"))?
    .value()
    .attr("href")
    .ok_or(anyhow!("no href found"))?
    .to_string();
  Ok(url)
}
