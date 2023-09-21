use gotrue_entity::OAuthProvider;
use gotrue_entity::OAuthURL;
use reqwest::Method;
use reqwest::RequestBuilder;
use shared_entity::data::AppResponse;
use shared_entity::dto::SignInParams;
use shared_entity::dto::UserUpdateParams;
use shared_entity::dto::WorkspaceMembersParams;
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
  http_client: reqwest::Client,
  base_url: String,
  ws_addr: String,
  token: ClientToken,
}

impl Client {
  pub fn from(c: reqwest::Client, base_url: &str, ws_addr: &str) -> Self {
    Self {
      base_url: base_url.to_string(),
      ws_addr: ws_addr.to_string(),
      http_client: c,
      token: ClientToken::new(),
    }
  }

  pub fn subscribe_token_state(&self) -> TokenStateReceiver {
    self.token.subscribe()
  }

  // e.g. appflowy-flutter://#access_token=...&expires_in=3600&provider_token=...&refresh_token=...&token_type=bearer
  pub async fn sign_in_url(&mut self, url: &str) -> Result<(), AppError> {
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
    let user = self.user_info(&access_token).await?;

    self.token.set(AccessTokenResponse {
      access_token,
      token_type: token_type.ok_or(url_missing_param("token_type"))?,
      expires_in: expires_in.ok_or(url_missing_param("expires_in"))?,
      expires_at: expires_at.ok_or(url_missing_param("expires_at"))?,
      refresh_token: refresh_token.ok_or(url_missing_param("refresh_token"))?,
      user,
      provider_access_token,
      provider_refresh_token,
    });

    Ok(())
  }

  pub async fn user_info(&self, access_token: &str) -> Result<User, AppError> {
    let url = format!("{}/api/user/info/{}", self.base_url, access_token);
    let resp = self.http_client.get(&url).send().await?;
    let user = AppResponse::<User>::from_response(resp)
      .await?
      .into_data()?;
    Ok(user)
  }

  pub fn token(&self) -> Option<&AccessTokenResponse> {
    self.token.as_ref()
  }

  pub async fn oauth_login(&self, provider: OAuthProvider) -> Result<(), AppError> {
    let url = format!("{}/api/user/oauth/{}", self.base_url, provider.as_str());
    let resp = self.http_client.get(&url).send().await?;
    let oauth_url = AppResponse::<OAuthURL>::from_response(resp)
      .await?
      .into_data()?;
    opener::open(oauth_url.url.as_str())?;
    Ok(())
  }

  pub async fn profile(&mut self) -> Result<AFUserProfileView, AppError> {
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

  pub async fn workspaces(&mut self) -> Result<AFWorkspaces, AppError> {
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
    &mut self,
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
    &mut self,
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
    &mut self,
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

  pub async fn sign_in_password(&mut self, email: &str, password: &str) -> Result<(), AppError> {
    let url = format!("{}/api/user/sign_in/password", self.base_url);
    let params = SignInParams {
      email: email.to_owned(),
      password: password.to_owned(),
    };
    let resp = self.http_client.post(&url).json(&params).send().await?;
    self
      .token
      .set(AppResponse::from_response(resp).await?.into_data()?);
    Ok(())
  }

  pub async fn refresh(&mut self) -> Result<(), AppError> {
    let refresh_token = self
      .token
      .as_ref()
      .ok_or::<AppError>(ErrorCode::NotLoggedIn.into())?
      .refresh_token
      .as_str();
    let url = format!("{}/api/user/refresh/{}", self.base_url, refresh_token);
    let resp = self.http_client.get(&url).send().await?;
    self
      .token
      .set(AppResponse::from_response(resp).await?.into_data()?);
    Ok(())
  }

  pub async fn sign_up(&self, email: &str, password: &str) -> Result<(), AppError> {
    let url = format!("{}/api/user/sign_up", self.base_url);
    let params = SignInParams {
      email: email.to_owned(),
      password: password.to_owned(),
    };
    let resp = self.http_client.post(&url).json(&params).send().await?;
    AppResponse::<()>::from_response(resp).await?.into_error()?;
    Ok(())
  }

  pub async fn sign_out(&mut self) -> Result<(), AppError> {
    let url = format!("{}/api/user/sign_out", self.base_url);
    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .send()
      .await?;
    AppResponse::<()>::from_response(resp).await?.into_error()?;
    self.token.unset();
    Ok(())
  }

  pub async fn update(
    &mut self,
    email: &str,
    password: &str,
    name: Option<&str>,
  ) -> Result<(), AppError> {
    let url = format!("{}/api/user/update", self.base_url);
    let params = UserUpdateParams {
      email: email.to_owned(),
      password: password.to_owned(),
      name: name.map(String::from),
    };
    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .json(&params)
      .send()
      .await?;
    let new_user = AppResponse::<User>::from_response(resp)
      .await?
      .into_data()?;
    if let Some(t) = self.token.as_mut() {
      t.user = new_user;
    }
    Ok(())
  }

  pub async fn create_collab(&mut self, params: InsertCollabParams) -> Result<(), AppError> {
    let url = format!("{}/api/collab/", self.base_url);
    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .json(&params)
      .send()
      .await?;
    AppResponse::<()>::from_response(resp).await?.into_error()
  }

  pub async fn update_collab(&mut self, params: InsertCollabParams) -> Result<(), AppError> {
    let url = format!("{}/api/collab/", self.base_url);
    let resp = self
      .http_client_with_auth(Method::PUT, &url)
      .await?
      .json(&params)
      .send()
      .await?;
    AppResponse::<()>::from_response(resp).await?.into_error()
  }

  pub async fn get_collab(&mut self, params: QueryCollabParams) -> Result<RawData, AppError> {
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

  pub async fn delete_collab(&mut self, params: DeleteCollabParams) -> Result<(), AppError> {
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
    match self.token() {
      None => Err(AppError::new(
        ErrorCode::OAuthError,
        "No token found".to_string(),
      )),
      Some(token) => Ok(format!(
        "{}/{}/{}",
        self.ws_addr, token.access_token, device_id
      )),
    }
  }

  async fn http_client_with_auth(
    &mut self,
    method: Method,
    url: &str,
  ) -> Result<RequestBuilder, AppError> {
    let token = self.token().ok_or(ErrorCode::NotLoggedIn)?;

    // Refresh token if it's about to expire
    let time_now_sec = SystemTime::now()
      .duration_since(SystemTime::UNIX_EPOCH)
      .unwrap()
      .as_secs() as i64;
    if time_now_sec + 60 > token.expires_at {
      // Add 60 seconds buffer
      self.refresh().await?;
    }

    let access_token = self
      .token()
      .ok_or(ErrorCode::NotLoggedIn)?
      .access_token
      .as_str();

    let request_builder = self
      .http_client
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
