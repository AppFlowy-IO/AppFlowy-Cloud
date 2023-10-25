use crate::notify::{ClientToken, TokenStateReceiver};
use anyhow::{anyhow, Context};
use bytes::Bytes;
use database_entity::dto::{
  AFBlobMetadata, AFBlobRecord, AFCollabMember, AFCollabMembers, AFUserProfile,
  AFUserWorkspaceInfo, AFWorkspace, AFWorkspaceMember, AFWorkspaces, BatchQueryCollabParams,
  BatchQueryCollabResult, CollabMemberIdentify, DeleteCollabParams, InsertCollabMemberParams,
  InsertCollabParams, QueryCollabMembers, QueryCollabParams, RawData, UpdateCollabMemberParams,
};
use futures_util::StreamExt;
use gotrue::grant::Grant;
use gotrue::grant::PasswordGrant;
use gotrue::grant::RefreshTokenGrant;
use gotrue::params::MagicLinkParams;
use gotrue::params::{AdminUserParams, GenerateLinkParams};
use mime::Mime;
use parking_lot::RwLock;
use reqwest::header;
use reqwest::Method;
use reqwest::RequestBuilder;
use scraper::{Html, Selector};
use shared_entity::app_error::AppError;
use shared_entity::data::AppResponse;
use shared_entity::dto::auth_dto::SignInTokenResponse;
use shared_entity::dto::auth_dto::UpdateUserParams;
use shared_entity::dto::workspace_dto::{
  CreateWorkspaceMembers, WorkspaceBlobMetadata, WorkspaceMemberChangeset, WorkspaceMembers,
  WorkspaceSpaceUsage,
};
use shared_entity::error_code::url_missing_param;
use shared_entity::error_code::ErrorCode;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use tracing::{event, instrument};
use url::Url;

use gotrue_entity::dto::SignUpResponse::{Authenticated, NotAuthenticated};
use gotrue_entity::dto::{AccessTokenResponse, OAuthProvider, UpdateGotrueUserParams, User};

/// `Client` is responsible for managing communication with the GoTrue API and cloud storage.
///
/// It provides methods to perform actions like signing in, signing out, refreshing tokens,
/// and interacting with file storage and collaboration objects.
///
/// # Fields
/// - `cloud_client`: A `reqwest::Client` used for HTTP requests to the cloud.
/// - `gotrue_client`: A `gotrue::api::Client` used for interacting with the GoTrue API.
/// - `base_url`: The base URL for API requests.
/// - `ws_addr`: The WebSocket address for real-time communication.
/// - `token`: An `Arc<RwLock<ClientToken>>` managing the client's authentication token.
///
pub struct Client {
  pub(crate) cloud_client: reqwest::Client,
  pub(crate) gotrue_client: gotrue::api::Client,
  base_url: String,
  ws_addr: String,
  token: Arc<RwLock<ClientToken>>,
}

/// Hardcoded schema in the frontend application. Do not change this value.
const DESKTOP_CALLBACK_URL: &str = "appflowy-flutter://login-callback";

impl Client {
  /// Constructs a new `Client` instance.
  ///
  /// # Parameters
  /// - `base_url`: The base URL for API requests.
  /// - `ws_addr`: The WebSocket address for real-time communication.
  /// - `gotrue_url`: The URL for the GoTrue API.
  pub fn new(base_url: &str, ws_addr: &str, gotrue_url: &str) -> Self {
    let reqwest_client = reqwest::Client::new();
    Self {
      base_url: base_url.to_string(),
      ws_addr: ws_addr.to_string(),
      cloud_client: reqwest_client.clone(),
      gotrue_client: gotrue::api::Client::new(reqwest_client, gotrue_url),
      token: Arc::new(RwLock::new(ClientToken::new())),
    }
  }

  #[instrument(level = "debug", skip_all, err)]
  pub fn restore_token(&self, token: &str) -> Result<(), AppError> {
    if token.is_empty() {
      return Err(AppError::new(ErrorCode::OAuthError, "Empty token"));
    }
    let token = serde_json::from_str::<AccessTokenResponse>(token)?;
    self.token.write().set(token);
    Ok(())
  }

  /// Retrieves the string representation of the [AccessTokenResponse]. The returned value can be
  /// saved to the client application's local storage and used to restore the client's authentication
  ///
  /// This function attempts to acquire a read lock on `self.token` and retrieves the
  /// string representation of the access token. If the lock cannot be acquired or
  /// the token is not present, an error is returned.
  #[instrument(level = "debug", skip_all, err)]
  pub fn get_token(&self) -> Result<String, AppError> {
    let token_str = self
      .token
      .read()
      .try_get()
      .map_err(|err| AppError::new(ErrorCode::OAuthError, err.to_string()))?;
    Ok(token_str)
  }

  pub fn subscribe_token_state(&self) -> TokenStateReceiver {
    self.token.read().subscribe()
  }

  /// Attempts to sign in using a URL, extracting and validating the token parameters from the URL fragment.
  /// It looks like, e.g., `appflowy-flutter://#access_token=...&expires_in=3600&provider_token=...&refresh_token=...&token_type=bearer`.
  ///
  pub async fn sign_in_with_url(&self, url: &str) -> Result<bool, AppError> {
    let mut access_token: Option<String> = None;
    let mut token_type: Option<String> = None;
    let mut expires_in: Option<i64> = None;
    let mut expires_at: Option<i64> = None;
    let mut refresh_token: Option<String> = None;
    let mut provider_access_token: Option<String> = None;
    let mut provider_refresh_token: Option<String> = None;

    Url::parse(url)?
      .fragment()
      .ok_or(url_missing_param("fragment"))?
      .split('&')
      .try_for_each(|f| -> Result<(), AppError> {
        let (k, v) = f.split_once('=').ok_or(url_missing_param("key=value"))?;
        match k {
          "access_token" => access_token = Some(v.to_string()),
          "token_type" => token_type = Some(v.to_string()),
          "expires_in" => expires_in = Some(v.parse::<i64>().context("parser expires_in failed")?),
          "expires_at" => expires_at = Some(v.parse::<i64>().context("parser expires_at failed")?),
          "refresh_token" => refresh_token = Some(v.to_string()),
          "provider_access_token" => provider_access_token = Some(v.to_string()),
          "provider_refresh_token" => provider_refresh_token = Some(v.to_string()),
          x => tracing::warn!("unhandled param in url: {}", x),
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

  /// Returns an OAuth URL by constructing the authorization URL for the specified provider.
  ///
  /// This asynchronous function communicates with the GoTrue client to retrieve settings and
  /// validate the availability of the specified OAuth provider. If the provider is available,
  /// it constructs and returns the OAuth URL. When the user opens the OAuth URL, it redirects to
  /// the corresponding provider's OAuth web page. After the user is authenticated, the browser will open
  /// a deep link to the AppFlowy app (iOS, macOS, etc.), which will call [Client::sign_in_with_url] to sign in.
  ///
  /// For example, the OAuth URL on Google looks like `https://appflowy.io/authorize?provider=google`.
  /// The deep link looks like `appflowy-flutter://#access_token=...&expires_in=3600&provider_token=...&refresh_token=...&token_type=bearer`.
  ///
  /// The appflowy-flutter:// is a hardcoded schema in the frontend application
  ///
  /// # Parameters
  /// - `provider`: A reference to an `OAuthProvider` indicating which OAuth provider to use for login.
  ///
  /// # Returns
  /// - `Ok(String)`: A `String` containing the constructed authorization URL if the specified provider is available.
  /// - `Err(AppError)`: An `AppError` indicating either the OAuth provider is invalid or other issues occurred while fetching settings.
  ///
  #[instrument(level = "debug", skip_all, err)]
  pub async fn generate_oauth_url_with_provider(
    &self,
    provider: &OAuthProvider,
  ) -> Result<String, AppError> {
    let settings = self.gotrue_client.settings().await?;
    if !settings.external.has_provider(provider) {
      return Err(ErrorCode::InvalidOAuthProvider.into());
    }

    let url = format!("{}/authorize", self.gotrue_client.base_url,);

    let mut url = Url::parse(&url)?;
    url
      .query_pairs_mut()
      .append_pair("provider", provider.as_str())
      .append_pair("redirect_to", DESKTOP_CALLBACK_URL);

    if let OAuthProvider::Google = provider {
      url
        .query_pairs_mut()
          // In many cases, especially for server-side applications or mobile apps that might need to
          // interact with Google services on behalf of the user without the user being actively
          // engaged, access_type=offline is preferred to ensure long-term access.
        .append_pair("access_type", "offline")
          // In Google OAuth2.0, the prompt parameter is used to control the OAuth2.0 flow's behavior.
          // It determines if the user is re-prompted for authentication and/or consent.
          // 1. none: The authorization server does not display any authentication or consent user interface pages.
          // 2. consent: The authorization server prompts the user for consent before returning information to the client
          // 3. select_account: The authorization server prompts the user to select a user account.
        .append_pair("prompt", "consent");
    }

    Ok(url.to_string())
  }

  /// Returns an OAuth URL by constructing the authorization URL for the specified provider.
  /// The URL looks like, e.g., `appflowy-flutter://#access_token=...&expires_in=3600&provider_token=...&refresh_token=...&token_type=bearer`.
  ///
  #[instrument(level = "debug", skip_all, err)]
  pub async fn generate_sign_in_url_with_email(
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
      .admin_generate_link(&admin_token.access_token, &admin_user_params)
      .await?;
    assert_eq!(link_resp.email, user_email);

    let action_link = link_resp.action_link;
    let resp = reqwest::Client::new().get(action_link).send().await?;
    let resp_text = resp.text().await?;
    Ok(extract_sign_in_url(&resp_text)?)
  }

  #[inline]
  async fn verify_token(&self, access_token: &str) -> Result<(User, bool), AppError> {
    let user = self.gotrue_client.user_info(access_token).await?;
    let is_new = self.verify_token_cloud(access_token).await?;
    Ok((user, is_new))
  }

  #[instrument(level = "debug", skip_all, err)]
  #[inline]
  async fn verify_token_cloud(&self, access_token: &str) -> Result<bool, AppError> {
    let url = format!("{}/api/user/verify/{}", self.base_url, access_token);
    let resp = self.cloud_client.get(&url).send().await?;
    let sign_in_resp: SignInTokenResponse = AppResponse::from_response(resp).await?.into_data()?;
    Ok(sign_in_resp.is_new)
  }

  // Invites another user by sending a magic link to the user's email address.
  #[instrument(level = "debug", skip_all, err)]
  pub async fn invite(&self, email: &str) -> Result<(), AppError> {
    self
      .gotrue_client
      .magic_link(
        &self.access_token()?,
        &MagicLinkParams {
          email: email.to_owned(),
          ..Default::default()
        },
      )
      .await?;
    Ok(())
  }

  #[instrument(level = "debug", skip_all, err)]
  pub async fn create_magic_link(&self, email: &str, password: &str) -> Result<User, AppError> {
    Ok(
      self
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
        .await?,
    )
  }

  #[instrument(level = "debug", skip_all, err)]
  pub async fn create_email_verified_user(
    &self,
    email: &str,
    password: &str,
  ) -> Result<User, AppError> {
    Ok(
      self
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
        .await?,
    )
  }

  /// Only expose this method for testing
  #[cfg(debug_assertions)]
  pub fn token(&self) -> Arc<RwLock<ClientToken>> {
    self.token.clone()
  }

  /// Retrieves the expiration timestamp of the current token.
  ///
  /// This function attempts to read the current token and, if successful, returns the expiration timestamp.
  ///
  /// # Returns
  /// - `Ok(i64)`: An `i64` representing the expiration timestamp of the token.
  /// - `Err(AppError)`: An `AppError` indicating either an inability to read the token or that the user is not logged in.
  ///
  #[inline]
  pub fn token_expires_at(&self) -> Result<i64, AppError> {
    match &self.token.try_read() {
      None => Err(AppError::new(ErrorCode::Unhandled, "Failed to read token")),
      Some(token) => Ok(
        token
          .as_ref()
          .ok_or(AppError::new(
            ErrorCode::NotLoggedIn,
            "fail to get expires_at",
          ))?
          .expires_at,
      ),
    }
  }

  /// Retrieves the access token string.
  ///
  /// This function attempts to read the current token and, if successful, returns the access token string.
  ///
  /// # Returns
  /// - `Ok(String)`: A `String` containing the access token.
  /// - `Err(AppError)`: An `AppError` indicating either an inability to read the token or that the user is not logged in.
  ///
  pub fn access_token(&self) -> Result<String, AppError> {
    match &self.token.try_read_for(Duration::from_secs(2)) {
      None => Err(AppError::new(ErrorCode::Unhandled, "Failed to read token")),
      Some(token) => Ok(
        token
          .as_ref()
          .ok_or(AppError::new(
            ErrorCode::NotLoggedIn,
            "fail to get access token. Token is empty",
          ))?
          .access_token
          .clone(),
      ),
    }
  }

  #[instrument(level = "debug", skip_all, err)]
  pub async fn get_profile(&self) -> Result<AFUserProfile, AppError> {
    let url = format!("{}/api/user/profile", self.base_url);
    let resp = self
      .http_client_with_auth(Method::GET, &url)
      .await?
      .send()
      .await?;
    AppResponse::<AFUserProfile>::from_response(resp)
      .await?
      .into_data()
  }

  #[instrument(level = "debug", skip_all, err)]
  pub async fn get_user_workspace_info(&self) -> Result<AFUserWorkspaceInfo, AppError> {
    let url = format!("{}/api/user/workspace", self.base_url);
    let resp = self
      .http_client_with_auth(Method::GET, &url)
      .await?
      .send()
      .await?;
    AppResponse::<AFUserWorkspaceInfo>::from_response(resp)
      .await?
      .into_data()
  }

  #[instrument(level = "debug", skip_all, err)]
  pub async fn get_workspaces(&self) -> Result<AFWorkspaces, AppError> {
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

  #[instrument(level = "debug", skip_all, err)]
  pub async fn open_workspace(&self, workspace_id: &str) -> Result<AFWorkspace, AppError> {
    let url = format!("{}/api/workspace/{}/open", self.base_url, workspace_id);
    let resp = self
      .http_client_with_auth(Method::PUT, &url)
      .await?
      .send()
      .await?;
    AppResponse::<AFWorkspace>::from_response(resp)
      .await?
      .into_data()
  }

  #[instrument(level = "debug", skip_all, err)]
  pub async fn get_workspace_members(
    &self,
    workspace_id: &str,
  ) -> Result<Vec<AFWorkspaceMember>, AppError> {
    let url = format!("{}/api/workspace/{}/member", self.base_url, workspace_id);
    let resp = self
      .http_client_with_auth(Method::GET, &url)
      .await?
      .send()
      .await?;
    AppResponse::<Vec<AFWorkspaceMember>>::from_response(resp)
      .await?
      .into_data()
  }

  #[instrument(level = "debug", skip_all, err)]
  pub async fn get_workspace_members2<W: AsRef<str>>(
    &self,
    workspace_id: W,
  ) -> Result<Vec<AFWorkspaceMember>, AppError> {
    let url = format!(
      "{}/api/workspace/{}/member",
      self.base_url,
      workspace_id.as_ref()
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

  #[instrument(level = "debug", skip_all, err)]
  pub async fn add_workspace_members<T: Into<CreateWorkspaceMembers>, W: AsRef<str>>(
    &self,
    workspace_id: W,
    members: T,
  ) -> Result<(), AppError> {
    let members = members.into();
    let url = format!(
      "{}/api/workspace/{}/member",
      self.base_url,
      workspace_id.as_ref()
    );
    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .json(&members)
      .send()
      .await?;
    AppResponse::<()>::from_response(resp).await?.into_error()?;
    Ok(())
  }

  #[instrument(level = "debug", skip_all, err)]
  pub async fn update_workspace_member<T: AsRef<str>>(
    &self,
    workspace_id: T,
    changeset: WorkspaceMemberChangeset,
  ) -> Result<(), AppError> {
    let url = format!(
      "{}/api/workspace/{}/member",
      self.base_url,
      workspace_id.as_ref()
    );
    let resp = self
      .http_client_with_auth(Method::PUT, &url)
      .await?
      .json(&changeset)
      .send()
      .await?;
    AppResponse::<()>::from_response(resp).await?.into_error()?;
    Ok(())
  }

  #[instrument(level = "debug", skip_all, err)]
  pub async fn remove_workspace_members<T: AsRef<str>>(
    &self,
    workspace_id: T,
    member_emails: Vec<String>,
  ) -> Result<(), AppError> {
    let url = format!(
      "{}/api/workspace/{}/member",
      self.base_url,
      workspace_id.as_ref()
    );
    let payload = WorkspaceMembers::from(member_emails);
    let resp = self
      .http_client_with_auth(Method::DELETE, &url)
      .await?
      .json(&payload)
      .send()
      .await?;
    AppResponse::<()>::from_response(resp).await?.into_error()?;
    Ok(())
  }

  // pub async fn update_workspace_member(&self, workspace_uuid: Uuid, member)

  #[instrument(skip_all, err)]
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

  /// Refreshes the access token using the stored refresh token.
  ///
  /// This function attempts to refresh the access token by sending a request to the authentication server
  /// using the stored refresh token. If successful, it updates the stored access token with the new one
  /// received from the server.
  #[instrument(level = "debug", skip_all, err)]
  pub async fn refresh(&self) -> Result<(), AppError> {
    let refresh_token = self
      .token
      .read()
      .as_ref()
      .ok_or(AppError::new(
        ErrorCode::NotLoggedIn,
        "fail to refresh user token",
      ))?
      .refresh_token
      .as_str()
      .to_owned();
    match self
      .gotrue_client
      .token(&Grant::RefreshToken(RefreshTokenGrant { refresh_token }))
      .await
    {
      Ok(access_token_resp) => {
        self.token.write().set(access_token_resp);
        Ok(())
      },
      Err(err) => {
        event!(tracing::Level::ERROR, "refresh token failed: {}", err);
        self.token.write().unset();
        Err(AppError::from(err))
      },
    }
  }

  #[instrument(level = "debug", skip_all, err)]
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

  #[instrument(level = "debug", skip_all, err)]
  pub async fn sign_out(&self) -> Result<(), AppError> {
    self.gotrue_client.logout(&self.access_token()?).await?;
    self.token.write().unset();
    Ok(())
  }

  #[instrument(level = "debug", skip_all, err)]
  pub async fn update_user(&self, params: UpdateUserParams) -> Result<(), AppError> {
    let gotrue_params = UpdateGotrueUserParams::new()
      .with_opt_email(params.email.clone())
      .with_opt_password(params.password.clone());

    let updated_user = self
      .gotrue_client
      .update_user(&self.access_token()?, &gotrue_params)
      .await?;

    if let Some(token) = self.token.write().as_mut() {
      token.user = updated_user;
    }

    let url = format!("{}/api/user/update", self.base_url);
    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .json(&params)
      .send()
      .await?;
    AppResponse::<()>::from_response(resp).await?.into_error()
  }

  #[instrument(level = "debug", skip_all, err)]
  pub async fn create_collab(&self, params: InsertCollabParams) -> Result<(), AppError> {
    let url = format!(
      "{}/api/workspace/{}/collab/{}",
      self.base_url, params.workspace_id, &params.object_id
    );
    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .json(&params)
      .send()
      .await?;
    AppResponse::<()>::from_response(resp).await?.into_error()
  }

  #[instrument(level = "debug", skip_all, err)]
  pub async fn update_collab(&self, params: InsertCollabParams) -> Result<(), AppError> {
    let url = format!(
      "{}/api/workspace/{}/collab/{}",
      self.base_url, &params.workspace_id, &params.object_id
    );
    let resp = self
      .http_client_with_auth(Method::PUT, &url)
      .await?
      .json(&params)
      .send()
      .await?;
    AppResponse::<()>::from_response(resp).await?.into_error()
  }

  #[instrument(level = "debug", skip_all, err)]
  pub async fn get_collab(&self, params: QueryCollabParams) -> Result<RawData, AppError> {
    let url = format!(
      "{}/api/workspace/{}/collab/{}",
      self.base_url, &params.workspace_id, &params.object_id
    );
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

  #[instrument(level = "debug", skip_all, err)]
  pub async fn batch_get_collab(
    &self,
    workspace_id: &str,
    params: BatchQueryCollabParams,
  ) -> Result<BatchQueryCollabResult, AppError> {
    let url = format!(
      "{}/api/workspace/{}/collab_list",
      self.base_url, workspace_id
    );
    let resp = self
      .http_client_with_auth(Method::GET, &url)
      .await?
      .json(&params)
      .send()
      .await?;
    AppResponse::<BatchQueryCollabResult>::from_response(resp)
      .await?
      .into_data()
  }

  #[instrument(level = "debug", skip_all, err)]
  pub async fn delete_collab(&self, params: DeleteCollabParams) -> Result<(), AppError> {
    let url = format!(
      "{}/api/workspace/{}/collab/{}",
      self.base_url, &params.workspace_id, &params.object_id
    );
    let resp = self
      .http_client_with_auth(Method::DELETE, &url)
      .await?
      .json(&params)
      .send()
      .await?;
    AppResponse::<()>::from_response(resp).await?.into_error()
  }

  #[instrument(level = "debug", skip_all, err)]
  pub async fn add_collab_member(&self, params: InsertCollabMemberParams) -> Result<(), AppError> {
    let url = format!(
      "{}/api/workspace/{}/collab/{}/member",
      self.base_url, params.workspace_id, &params.object_id
    );
    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .json(&params)
      .send()
      .await?;
    AppResponse::<()>::from_response(resp).await?.into_error()
  }

  #[instrument(level = "debug", skip_all, err)]
  pub async fn get_collab_member(
    &self,
    params: CollabMemberIdentify,
  ) -> Result<AFCollabMember, AppError> {
    let url = format!(
      "{}/api/workspace/{}/collab/{}/member",
      self.base_url, params.workspace_id, &params.object_id
    );
    let resp = self
      .http_client_with_auth(Method::GET, &url)
      .await?
      .json(&params)
      .send()
      .await?;
    AppResponse::<AFCollabMember>::from_response(resp)
      .await?
      .into_data()
  }

  #[instrument(level = "debug", skip_all, err)]
  pub async fn update_collab_member(
    &self,
    params: UpdateCollabMemberParams,
  ) -> Result<(), AppError> {
    let url = format!(
      "{}/api/workspace/{}/collab/{}/member",
      self.base_url, params.workspace_id, &params.object_id
    );
    let resp = self
      .http_client_with_auth(Method::PUT, &url)
      .await?
      .json(&params)
      .send()
      .await?;
    AppResponse::<()>::from_response(resp).await?.into_error()
  }

  #[instrument(level = "debug", skip_all, err)]
  pub async fn remove_collab_member(&self, params: CollabMemberIdentify) -> Result<(), AppError> {
    let url = format!(
      "{}/api/workspace/{}/collab/{}/member",
      self.base_url, params.workspace_id, &params.object_id
    );
    let resp = self
      .http_client_with_auth(Method::DELETE, &url)
      .await?
      .json(&params)
      .send()
      .await?;
    AppResponse::<()>::from_response(resp).await?.into_error()
  }

  #[instrument(level = "debug", skip_all, err)]
  pub async fn get_collab_members(
    &self,
    params: QueryCollabMembers,
  ) -> Result<AFCollabMembers, AppError> {
    let url = format!(
      "{}/api/workspace/{}/collab/{}/member/list",
      self.base_url, params.workspace_id, &params.object_id
    );
    let resp = self
      .http_client_with_auth(Method::GET, &url)
      .await?
      .json(&params)
      .send()
      .await?;
    AppResponse::<AFCollabMembers>::from_response(resp)
      .await?
      .into_data()
  }

  pub fn ws_url(&self, device_id: &str) -> Result<String, AppError> {
    let access_token = self.access_token()?;
    Ok(format!("{}/{}/{}", self.ws_addr, access_token, device_id))
  }

  pub async fn put_blob<T: Into<Bytes>, M: ToString>(
    &self,
    workspace_id: &str,
    data: T,
    mime: M,
  ) -> Result<String, AppError> {
    let url = format!("{}/api/file_storage/{}/blob", self.base_url, workspace_id);
    let data = data.into();
    let content_length = data.len();
    let resp = self
      .http_client_with_auth(Method::PUT, &url)
      .await?
      .header(header::CONTENT_TYPE, mime.to_string())
      .header(header::CONTENT_LENGTH, content_length)
      .body(data)
      .send()
      .await?;
    let record = AppResponse::<AFBlobRecord>::from_response(resp)
      .await?
      .into_data()?;
    Ok(format!(
      "{}/api/file_storage/{}/blob/{}",
      self.base_url, workspace_id, record.file_id
    ))
  }

  pub async fn put_blob_with_path(
    &self,
    workspace_id: &str,
    file_path: &str,
  ) -> Result<String, AppError> {
    if file_path.is_empty() {
      return Err(AppError::new(
        ErrorCode::InvalidRequestParams,
        "path is empty",
      ));
    }

    let mut file = File::open(&file_path).await?;
    let mime = mime_guess::from_path(file_path)
      .first_or_octet_stream()
      .to_string();

    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer).await?;
    self.put_blob(workspace_id, buffer, mime).await
  }

  /// Only expose this method for testing
  #[cfg(debug_assertions)]
  pub async fn put_blob_with_content_length<T: Into<Bytes>>(
    &self,
    workspace_id: &str,
    data: T,
    mime: &Mime,
    content_length: usize,
  ) -> Result<AFBlobRecord, AppError> {
    let url = format!("{}/api/file_storage/{}/blob", self.base_url, workspace_id);
    let resp = self
      .http_client_with_auth(Method::PUT, &url)
      .await?
      .header(header::CONTENT_TYPE, mime.to_string())
      .header(header::CONTENT_LENGTH, content_length)
      .body(data.into())
      .send()
      .await?;
    AppResponse::<AFBlobRecord>::from_response(resp)
      .await?
      .into_data()
  }

  /// Get the file with the given url. The url should be in the format of
  /// `https://appflowy.io/api/file_storage/<workspace_id>/<file_id>`.
  pub async fn get_blob<T: AsRef<str>>(&self, url: T) -> Result<Bytes, AppError> {
    Url::parse(url.as_ref())?;
    let resp = self
      .http_client_with_auth(Method::GET, url.as_ref())
      .await?
      .send()
      .await?;

    match resp.status() {
      reqwest::StatusCode::OK => {
        let mut stream = resp.bytes_stream();
        let mut acc: Vec<u8> = Vec::new();
        while let Some(raw_bytes) = stream.next().await {
          acc.extend_from_slice(&raw_bytes?);
        }
        Ok(Bytes::from(acc))
      },
      reqwest::StatusCode::NOT_FOUND => Err(ErrorCode::RecordNotFound.into()),
      c => Err(AppError::new(
        ErrorCode::Unhandled,
        format!("status code: {}, message: {}", c, resp.text().await?),
      )),
    }
  }

  pub async fn get_blob_metadata<T: AsRef<str>>(&self, url: T) -> Result<AFBlobMetadata, AppError> {
    let resp = self
      .http_client_with_auth(Method::GET, url.as_ref())
      .await?
      .send()
      .await?;

    AppResponse::<AFBlobMetadata>::from_response(resp)
      .await?
      .into_data()
  }

  pub async fn delete_blob(&self, url: &str) -> Result<(), AppError> {
    let resp = self
      .http_client_with_auth(Method::DELETE, url)
      .await?
      .send()
      .await?;
    AppResponse::<()>::from_response(resp).await?.into_error()
  }

  pub async fn get_workspace_usage(
    &self,
    workspace_id: &str,
  ) -> Result<WorkspaceSpaceUsage, AppError> {
    let url = format!("{}/api/file_storage/{}/usage", self.base_url, workspace_id);
    let resp = self
      .http_client_with_auth(Method::GET, &url)
      .await?
      .send()
      .await?;
    AppResponse::<WorkspaceSpaceUsage>::from_response(resp)
      .await?
      .into_data()
  }

  pub async fn get_workspace_all_blob_metadata(
    &self,
    workspace_id: &str,
  ) -> Result<WorkspaceBlobMetadata, AppError> {
    let url = format!("{}/api/file_storage/{}/blobs", self.base_url, workspace_id);
    let resp = self
      .http_client_with_auth(Method::GET, &url)
      .await?
      .send()
      .await?;
    AppResponse::<WorkspaceBlobMetadata>::from_response(resp)
      .await?
      .into_data()
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
    .ok_or(anyhow!("no a tag found in html: {}", html_str))?
    .value()
    .attr("href")
    .ok_or(anyhow!("no href found in html: {}", html_str))?
    .to_string();
  Ok(url)
}
