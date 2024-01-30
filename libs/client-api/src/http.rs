use crate::notify::{ClientToken, TokenStateReceiver};
use anyhow::Context;
use brotli::CompressorReader;
use gotrue_entity::dto::AuthProvider;
use shared_entity::dto::workspace_dto::CreateWorkspaceParam;
use std::fmt::{Display, Formatter};
use std::io::Read;

use app_error::AppError;
use bytes::Bytes;
use database_entity::dto::{
  AFCollabMember, AFCollabMembers, AFSnapshotMeta, AFSnapshotMetas, AFUserProfile,
  AFUserWorkspaceInfo, AFWorkspace, AFWorkspaceMember, AFWorkspaces, BatchQueryCollabParams,
  BatchQueryCollabResult, CollabMemberIdentify, CreateCollabParams, DeleteCollabParams,
  InsertCollabMemberParams, QueryCollab, QueryCollabMembers, QueryCollabParams,
  QuerySnapshotParams, SnapshotData, UpdateCollabMemberParams,
};
use futures_util::StreamExt;
use gotrue::grant::Grant;
use gotrue::grant::PasswordGrant;

use gotrue::params::MagicLinkParams;
use gotrue::params::{AdminUserParams, GenerateLinkParams};
use mime::Mime;
use parking_lot::RwLock;
use realtime_entity::EncodedCollab;
use reqwest::{header, StatusCode};

use collab_entity::CollabType;
use reqwest::header::HeaderValue;
use reqwest::Method;
use reqwest::RequestBuilder;
use shared_entity::dto::auth_dto::SignInTokenResponse;
use shared_entity::dto::auth_dto::UpdateUserParams;
use shared_entity::dto::workspace_dto::{
  BlobMetadata, CreateWorkspaceMembers, RepeatedBlobMetaData, WorkspaceMemberChangeset,
  WorkspaceMembers, WorkspaceSpaceUsage,
};
use shared_entity::response::{AppResponse, AppResponseError};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;
use tracing::{error, event, instrument, trace, warn};
use url::Url;

use gotrue_entity::dto::SignUpResponse::{Authenticated, NotAuthenticated};
use gotrue_entity::dto::{GotrueTokenResponse, UpdateGotrueUserParams, User};

pub const CLIENT_API_VERSION: Option<&'static str> = std::option_env!("GITHUB_SHA");
pub const X_COMPRESSION_TYPE: &str = "X-Compression-Type";
pub const X_COMPRESSION_BUFFER_SIZE: &str = "X-Compression-Buffer-Size";
pub const X_COMPRESSION_TYPE_BROTLI: &str = "brotli";

#[derive(Clone)]
pub struct ClientConfiguration {
  /// Lower Levels (0-4): Faster compression and decompression speeds but lower compression ratios. Suitable for scenarios where speed is more critical than reducing data size.
  /// Medium Levels (5-9): A balance between compression ratio and speed. These levels are generally good for a mix of performance and efficiency.
  /// Higher Levels (10-11): The highest compression ratios, but significantly slower and more resource-intensive. These are typically used in scenarios where reducing data size is paramount and resource usage is a secondary concern, such as for static content compression in web servers.
  pub(crate) compression_quality: u32,
  /// A larger buffer size means more data is compressed in a single operation, which can lead to better compression ratios
  /// since Brotli has more data to analyze for patterns and repetitions.
  pub(crate) compression_buffer_size: usize,
}

impl ClientConfiguration {
  pub fn with_compression_buffer_size(mut self, compression_buffer_size: usize) -> Self {
    self.compression_buffer_size = compression_buffer_size;
    self
  }

  pub fn with_compression_quality(mut self, compression_quality: u32) -> Self {
    self.compression_quality = if compression_quality > 11 {
      warn!("compression_quality is larger than 11, set it to 11");
      11
    } else {
      compression_quality
    };
    self
  }
}

impl Default for ClientConfiguration {
  fn default() -> Self {
    Self {
      compression_quality: 8,
      compression_buffer_size: 10240,
    }
  }
}

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
#[derive(Clone)]
pub struct Client {
  pub(crate) cloud_client: reqwest::Client,
  pub(crate) gotrue_client: gotrue::api::Client,
  pub base_url: String,
  ws_addr: String,
  pub(crate) token: Arc<RwLock<ClientToken>>,
  pub(crate) is_refreshing_token: Arc<AtomicBool>,
  pub(crate) refresh_ret_txs: Arc<RwLock<Vec<RefreshTokenSender>>>,
  pub(crate) config: ClientConfiguration,
}

pub(crate) type RefreshTokenRet = tokio::sync::oneshot::Receiver<Result<(), AppResponseError>>;
pub(crate) type RefreshTokenSender = tokio::sync::oneshot::Sender<Result<(), AppResponseError>>;

/// Hardcoded schema in the frontend application. Do not change this value.
const DESKTOP_CALLBACK_URL: &str = "appflowy-flutter://login-callback";

impl Client {
  /// Constructs a new `Client` instance.
  ///
  /// # Parameters
  /// - `base_url`: The base URL for API requests.
  /// - `ws_addr`: The WebSocket address for real-time communication.
  /// - `gotrue_url`: The URL for the GoTrue API.
  pub fn new(base_url: &str, ws_addr: &str, gotrue_url: &str, config: ClientConfiguration) -> Self {
    let reqwest_client = reqwest::Client::new();
    Self {
      base_url: base_url.to_string(),
      ws_addr: ws_addr.to_string(),
      cloud_client: reqwest_client.clone(),
      gotrue_client: gotrue::api::Client::new(reqwest_client, gotrue_url),
      token: Arc::new(RwLock::new(ClientToken::new())),
      is_refreshing_token: Default::default(),
      refresh_ret_txs: Default::default(),
      config,
    }
  }

  pub fn base_url(&self) -> &str {
    &self.base_url
  }

  pub fn ws_addr(&self) -> &str {
    &self.ws_addr
  }

  pub fn gotrue_url(&self) -> &str {
    &self.gotrue_client.base_url
  }

  #[instrument(level = "debug", skip_all, err)]
  pub fn restore_token(&self, token: &str) -> Result<(), AppResponseError> {
    if token.is_empty() {
      return Err(AppError::OAuthError("Empty token".to_string()).into());
    }
    let token = serde_json::from_str::<GotrueTokenResponse>(token)?;
    self.token.write().set(token);
    Ok(())
  }

  /// Retrieves the string representation of the [GotrueTokenResponse]. The returned value can be
  /// saved to the client application's local storage and used to restore the client's authentication
  ///
  /// This function attempts to acquire a read lock on `self.token` and retrieves the
  /// string representation of the access token. If the lock cannot be acquired or
  /// the token is not present, an error is returned.
  #[instrument(level = "debug", skip_all, err)]
  pub fn get_token(&self) -> Result<String, AppResponseError> {
    let token_str = self
      .token
      .read()
      .try_get()
      .map_err(|err| AppResponseError::from(AppError::OAuthError(err.to_string())))?;
    Ok(token_str)
  }

  pub fn subscribe_token_state(&self) -> TokenStateReceiver {
    self.token.read().subscribe()
  }

  /// Attempts to sign in using a URL, extracting and validating the token parameters from the URL fragment.
  /// It looks like, e.g., `appflowy-flutter://#access_token=...&expires_in=3600&provider_token=...&refresh_token=...&token_type=bearer`.
  ///
  /// return a bool indicating if the user is new
  #[instrument(level = "debug", skip_all, err)]
  pub async fn sign_in_with_url(&self, url: &str) -> Result<bool, AppResponseError> {
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
      .try_for_each(|f| -> Result<(), AppResponseError> {
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
    if access_token.is_empty() {
      return Err(AppError::OAuthError("Empty access token".to_string()).into());
    }

    let (user, new) = self.verify_token(&access_token).await?;
    self.token.write().set(GotrueTokenResponse {
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
  /// - `Err(AppResponseError)`: An `AppResponseError` indicating either the OAuth provider is invalid or other issues occurred while fetching settings.
  ///
  #[instrument(level = "debug", skip_all, err)]
  pub async fn generate_oauth_url_with_provider(
    &self,
    provider: &AuthProvider,
  ) -> Result<String, AppResponseError> {
    let settings = self.gotrue_client.settings().await?;
    if !settings.external.has_provider(provider) {
      return Err(AppError::InvalidOAuthProvider(provider.as_str().to_owned()).into());
    }

    let url = format!("{}/authorize", self.gotrue_client.base_url,);

    let mut url = Url::parse(&url)?;
    url
      .query_pairs_mut()
      .append_pair("provider", provider.as_str())
      .append_pair("redirect_to", DESKTOP_CALLBACK_URL);

    if let AuthProvider::Google = provider {
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

  /// Generates a sign action link for the specified email address.
  /// This is only applicable if user token is with admin privilege.
  /// This action link is used on web browser to sign in. When user then click the action link in the browser,
  /// which calls gotrue authentication server, which then redirects to the appflowy-flutter:// with the authentication token.
  ///
  /// [Self::extract_sign_in_url] simulates the browser behavior to extract the sign in url.
  ///
  #[instrument(level = "debug", skip_all, err)]
  pub async fn generate_sign_in_action_link(
    &self,
    email: &str,
  ) -> Result<String, AppResponseError> {
    let admin_user_params: GenerateLinkParams = GenerateLinkParams {
      email: email.to_string(),
      ..Default::default()
    };

    let link_resp = self
      .gotrue_client
      .admin_generate_link(&self.access_token()?, &admin_user_params)
      .await?;
    assert_eq!(link_resp.email, email);

    Ok(link_resp.action_link)
  }

  #[cfg(feature = "test_util")]
  /// Used to extract the sign in url from the action link
  /// Only expose this method for testing
  pub async fn extract_sign_in_url(&self, action_link: &str) -> Result<String, AppResponseError> {
    let resp = reqwest::Client::new().get(action_link).send().await?;
    let html = resp.text().await.unwrap();

    trace!("action_link:{}, html: {}", action_link, html);
    let fragment = scraper::Html::parse_fragment(&html);
    let selector = scraper::Selector::parse("a").unwrap();
    let sign_in_url = fragment
      .select(&selector)
      .next()
      .unwrap()
      .value()
      .attr("href")
      .unwrap()
      .to_string();

    Ok(sign_in_url)
  }

  #[inline]
  #[instrument(level = "debug", skip_all, err)]
  async fn verify_token(&self, access_token: &str) -> Result<(User, bool), AppResponseError> {
    let user = self.gotrue_client.user_info(access_token).await?;
    let is_new = self.verify_token_cloud(access_token).await?;
    Ok((user, is_new))
  }

  #[instrument(level = "debug", skip_all, err)]
  #[inline]
  async fn verify_token_cloud(&self, access_token: &str) -> Result<bool, AppResponseError> {
    let url = format!("{}/api/user/verify/{}", self.base_url, access_token);
    let resp = self.cloud_client.get(&url).send().await?;
    let sign_in_resp: SignInTokenResponse = AppResponse::from_response(resp).await?.into_data()?;
    Ok(sign_in_resp.is_new)
  }

  // Invites another user by sending a magic link to the user's email address.
  #[instrument(level = "debug", skip_all, err)]
  pub async fn invite(&self, email: &str) -> Result<(), AppResponseError> {
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
  pub async fn create_magic_link(
    &self,
    email: &str,
    password: &str,
  ) -> Result<User, AppResponseError> {
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
  ) -> Result<User, AppResponseError> {
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
  /// - `Ok(i64)`: An `i64` representing the expiration timestamp of the token in seconds.
  /// - `Err(AppError)`: An `AppError` indicating either an inability to read the token or that the user is not logged in.
  ///
  #[inline]
  pub fn token_expires_at(&self) -> Result<i64, AppResponseError> {
    match &self.token.try_read() {
      None => Err(AppError::Unhandled("Failed to read token".to_string()).into()),
      Some(token) => Ok(
        token
          .as_ref()
          .ok_or(AppResponseError::from(AppError::NotLoggedIn(
            "token is empty".to_string(),
          )))?
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
  /// - `Err(AppResponseError)`: An `AppResponseError` indicating either an inability to read the token or that the user is not logged in.
  ///
  pub fn access_token(&self) -> Result<String, AppResponseError> {
    match &self.token.try_read_for(Duration::from_secs(2)) {
      None => Err(AppError::Unhandled("Failed to read token".to_string()).into()),
      Some(token) => {
        let access_token = token
          .as_ref()
          .ok_or(AppResponseError::from(AppError::NotLoggedIn(
            "fail to get access token. Token is empty".to_string(),
          )))?
          .access_token
          .clone();

        if access_token.is_empty() {
          error!("Unexpected empty access token");
        }
        Ok(access_token)
      },
    }
  }

  #[instrument(level = "debug", skip_all, err)]
  pub async fn get_profile(&self) -> Result<AFUserProfile, AppResponseError> {
    let url = format!("{}/api/user/profile", self.base_url);
    let resp = self
      .http_client_with_auth(Method::GET, &url)
      .await?
      .send()
      .await?;
    log_request_id(&resp);
    AppResponse::<AFUserProfile>::from_response(resp)
      .await?
      .into_data()
  }

  #[instrument(level = "debug", skip_all, err)]
  pub async fn get_user_workspace_info(&self) -> Result<AFUserWorkspaceInfo, AppResponseError> {
    let url = format!("{}/api/user/workspace", self.base_url);
    let resp = self
      .http_client_with_auth(Method::GET, &url)
      .await?
      .send()
      .await?;
    log_request_id(&resp);
    AppResponse::<AFUserWorkspaceInfo>::from_response(resp)
      .await?
      .into_data()
  }

  #[instrument(level = "debug", skip_all, err)]
  pub async fn delete_workspace(&self, workspace_id: &str) -> Result<(), AppResponseError> {
    let url = format!("{}/api/workspace/{}", self.base_url, workspace_id);
    let resp = self
      .http_client_with_auth(Method::DELETE, &url)
      .await?
      .send()
      .await?;
    log_request_id(&resp);
    AppResponse::<()>::from_response(resp).await?.into_error()?;
    Ok(())
  }

  #[instrument(level = "debug", skip_all, err)]
  pub async fn create_workspace(
    &self,
    params: CreateWorkspaceParam,
  ) -> Result<AFWorkspace, AppResponseError> {
    let url = format!("{}/api/workspace", self.base_url);
    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .json(&params)
      .send()
      .await?;
    log_request_id(&resp);
    AppResponse::<AFWorkspace>::from_response(resp)
      .await?
      .into_data()
  }

  #[instrument(level = "debug", skip_all, err)]
  pub async fn get_workspaces(&self) -> Result<AFWorkspaces, AppResponseError> {
    let url = format!("{}/api/workspace", self.base_url);
    let resp = self
      .http_client_with_auth(Method::GET, &url)
      .await?
      .send()
      .await?;
    log_request_id(&resp);
    AppResponse::<AFWorkspaces>::from_response(resp)
      .await?
      .into_data()
  }

  #[instrument(level = "debug", skip_all, err)]
  pub async fn open_workspace(&self, workspace_id: &str) -> Result<AFWorkspace, AppResponseError> {
    let url = format!("{}/api/workspace/{}/open", self.base_url, workspace_id);
    let resp = self
      .http_client_with_auth(Method::PUT, &url)
      .await?
      .send()
      .await?;
    log_request_id(&resp);
    AppResponse::<AFWorkspace>::from_response(resp)
      .await?
      .into_data()
  }

  #[instrument(level = "debug", skip_all, err)]
  pub async fn get_workspace_members<W: AsRef<str>>(
    &self,
    workspace_id: W,
  ) -> Result<Vec<AFWorkspaceMember>, AppResponseError> {
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
    log_request_id(&resp);
    AppResponse::<Vec<AFWorkspaceMember>>::from_response(resp)
      .await?
      .into_data()
  }

  #[instrument(level = "debug", skip_all, err)]
  pub async fn add_workspace_members<T: Into<CreateWorkspaceMembers>, W: AsRef<str>>(
    &self,
    workspace_id: W,
    members: T,
  ) -> Result<(), AppResponseError> {
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
    log_request_id(&resp);
    AppResponse::<()>::from_response(resp).await?.into_error()?;
    Ok(())
  }

  #[instrument(level = "debug", skip_all, err)]
  pub async fn update_workspace_member<T: AsRef<str>>(
    &self,
    workspace_id: T,
    changeset: WorkspaceMemberChangeset,
  ) -> Result<(), AppResponseError> {
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
    log_request_id(&resp);
    AppResponse::<()>::from_response(resp).await?.into_error()?;
    Ok(())
  }

  #[instrument(level = "debug", skip_all, err)]
  pub async fn remove_workspace_members<T: AsRef<str>>(
    &self,
    workspace_id: T,
    member_emails: Vec<String>,
  ) -> Result<(), AppResponseError> {
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
    log_request_id(&resp);
    AppResponse::<()>::from_response(resp).await?.into_error()?;
    Ok(())
  }

  #[instrument(skip_all, err)]
  pub async fn sign_in_password(
    &self,
    email: &str,
    password: &str,
  ) -> Result<bool, AppResponseError> {
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

  #[instrument(level = "debug", skip_all, err)]
  pub async fn sign_up(&self, email: &str, password: &str) -> Result<(), AppResponseError> {
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

  #[instrument(level = "debug", skip_all)]
  pub async fn sign_out(&self) -> Result<(), AppResponseError> {
    self.gotrue_client.logout(&self.access_token()?).await?;
    self.token.write().unset();
    Ok(())
  }

  #[instrument(level = "debug", skip_all, err)]
  pub async fn update_user(&self, params: UpdateUserParams) -> Result<(), AppResponseError> {
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
    log_request_id(&resp);
    AppResponse::<()>::from_response(resp).await?.into_error()
  }

  #[instrument(level = "debug", skip_all, err)]
  pub async fn create_collab(&self, params: CreateCollabParams) -> Result<(), AppResponseError> {
    let url = format!(
      "{}/api/workspace/{}/collab/{}",
      self.base_url, params.workspace_id, &params.object_id
    );
    let bytes = params
      .to_bytes()
      .map_err(|err| AppError::Internal(err.into()))?;

    let compress_bytes = spawn_blocking_brotli_compress(
      bytes,
      self.config.compression_quality,
      self.config.compression_buffer_size,
    )
    .await?;

    #[allow(unused_mut)]
    let mut builder = self
      .http_client_with_auth_compress(Method::POST, &url)
      .await?;

    #[cfg(not(target_arch = "wasm32"))]
    {
      builder = builder.timeout(Duration::from_secs(60));
    }

    let resp = builder.body(compress_bytes).send().await?;
    log_request_id(&resp);
    AppResponse::<()>::from_response(resp).await?.into_error()
  }

  pub async fn get_snapshot_list(
    &self,
    workspace_id: &str,
    object_id: &str,
  ) -> Result<AFSnapshotMetas, AppResponseError> {
    let url = format!(
      "{}/api/workspace/{}/{}/snapshot/list",
      self.base_url, workspace_id, object_id
    );
    let resp = self
      .http_client_with_auth(Method::GET, &url)
      .await?
      .send()
      .await?;
    log_request_id(&resp);
    AppResponse::<AFSnapshotMetas>::from_response(resp)
      .await?
      .into_data()
  }

  pub async fn get_snapshot(
    &self,
    workspace_id: &str,
    object_id: &str,
    params: QuerySnapshotParams,
  ) -> Result<SnapshotData, AppResponseError> {
    let url = format!(
      "{}/api/workspace/{}/{}/snapshot",
      self.base_url, workspace_id, object_id,
    );
    let resp = self
      .http_client_with_auth(Method::GET, &url)
      .await?
      .json(&params)
      .send()
      .await?;
    log_request_id(&resp);
    AppResponse::<SnapshotData>::from_response(resp)
      .await?
      .into_data()
  }

  pub async fn create_snapshot(
    &self,
    workspace_id: &str,
    object_id: &str,
    collab_type: CollabType,
  ) -> Result<AFSnapshotMeta, AppResponseError> {
    let url = format!(
      "{}/api/workspace/{}/{}/snapshot",
      self.base_url, workspace_id, object_id,
    );
    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .json(&collab_type)
      .send()
      .await?;
    log_request_id(&resp);
    AppResponse::<AFSnapshotMeta>::from_response(resp)
      .await?
      .into_data()
  }

  #[instrument(level = "debug", skip_all, err)]
  pub async fn update_collab(&self, params: CreateCollabParams) -> Result<(), AppResponseError> {
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
    log_request_id(&resp);
    AppResponse::<()>::from_response(resp).await?.into_error()
  }

  #[instrument(level = "debug", skip_all)]
  pub async fn get_collab(
    &self,
    params: QueryCollabParams,
  ) -> Result<EncodedCollab, AppResponseError> {
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
    log_request_id(&resp);
    AppResponse::<EncodedCollab>::from_response(resp)
      .await?
      .into_data()
  }

  #[instrument(level = "debug", skip_all, err)]
  pub async fn batch_get_collab(
    &self,
    workspace_id: &str,
    params: Vec<QueryCollab>,
  ) -> Result<BatchQueryCollabResult, AppResponseError> {
    let url = format!(
      "{}/api/workspace/{}/collab_list",
      self.base_url, workspace_id
    );
    let params = BatchQueryCollabParams(params);
    let resp = self
      .http_client_with_auth(Method::GET, &url)
      .await?
      .json(&params)
      .send()
      .await?;
    log_request_id(&resp);
    AppResponse::<BatchQueryCollabResult>::from_response(resp)
      .await?
      .into_data()
  }

  #[instrument(level = "debug", skip_all, err)]
  pub async fn delete_collab(&self, params: DeleteCollabParams) -> Result<(), AppResponseError> {
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
    log_request_id(&resp);
    AppResponse::<()>::from_response(resp).await?.into_error()
  }

  #[instrument(level = "debug", skip_all, err)]
  pub async fn add_collab_member(
    &self,
    params: InsertCollabMemberParams,
  ) -> Result<(), AppResponseError> {
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
    log_request_id(&resp);
    AppResponse::<()>::from_response(resp).await?.into_error()
  }

  #[instrument(level = "debug", skip_all, err)]
  pub async fn get_collab_member(
    &self,
    params: CollabMemberIdentify,
  ) -> Result<AFCollabMember, AppResponseError> {
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
    log_request_id(&resp);
    AppResponse::<AFCollabMember>::from_response(resp)
      .await?
      .into_data()
  }

  #[instrument(level = "debug", skip_all, err)]
  pub async fn update_collab_member(
    &self,
    params: UpdateCollabMemberParams,
  ) -> Result<(), AppResponseError> {
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
    log_request_id(&resp);
    AppResponse::<()>::from_response(resp).await?.into_error()
  }

  #[instrument(level = "debug", skip_all, err)]
  pub async fn remove_collab_member(
    &self,
    params: CollabMemberIdentify,
  ) -> Result<(), AppResponseError> {
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
    log_request_id(&resp);
    AppResponse::<()>::from_response(resp).await?.into_error()
  }

  #[instrument(level = "debug", skip_all, err)]
  pub async fn get_collab_members(
    &self,
    params: QueryCollabMembers,
  ) -> Result<AFCollabMembers, AppResponseError> {
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
    log_request_id(&resp);
    AppResponse::<AFCollabMembers>::from_response(resp)
      .await?
      .into_data()
  }

  pub async fn ws_url(&self, device_id: &str) -> Result<String, AppResponseError> {
    self
      .refresh_if_expired(chrono::Local::now().timestamp())
      .await?;

    let access_token = self.access_token()?;
    Ok(format!("{}/{}/{}", self.ws_addr, access_token, device_id))
  }

  pub fn get_blob_url(&self, workspace_id: &str, file_id: &str) -> String {
    format!(
      "{}/api/file_storage/{}/blob/{}",
      self.base_url, workspace_id, file_id
    )
  }

  pub async fn put_blob<T: Into<Bytes>>(
    &self,
    url: &str,
    data: T,
    mime: &Mime,
  ) -> Result<(), AppResponseError> {
    let data = data.into();
    let resp = self
      .http_client_with_auth(Method::PUT, url)
      .await?
      .header(header::CONTENT_TYPE, mime.to_string())
      .body(data)
      .send()
      .await?;
    log_request_id(&resp);
    if resp.status() == StatusCode::PAYLOAD_TOO_LARGE {
      return Err(AppResponseError::from(AppError::PayloadTooLarge(
        StatusCode::PAYLOAD_TOO_LARGE.to_string(),
      )));
    }
    AppResponse::<()>::from_response(resp).await?.into_error()
  }

  /// Only expose this method for testing
  #[cfg(debug_assertions)]
  pub async fn put_blob_with_content_length<T: Into<Bytes>>(
    &self,
    url: &str,
    data: T,
    mime: &Mime,
    content_length: usize,
  ) -> Result<crate::entity::AFBlobRecord, AppResponseError> {
    let resp = self
      .http_client_with_auth(Method::PUT, url)
      .await?
      .header(header::CONTENT_TYPE, mime.to_string())
      .header(header::CONTENT_LENGTH, content_length)
      .body(data.into())
      .send()
      .await?;
    log_request_id(&resp);
    AppResponse::<crate::entity::AFBlobRecord>::from_response(resp)
      .await?
      .into_data()
  }

  /// Get the file with the given url. The url should be in the format of
  /// `https://appflowy.io/api/file_storage/<workspace_id>/<file_id>`.
  pub async fn get_blob(&self, url: &str) -> Result<(Mime, Vec<u8>), AppResponseError> {
    let resp = self
      .http_client_with_auth(Method::GET, url)
      .await?
      .send()
      .await?;
    log_request_id(&resp);

    match resp.status() {
      reqwest::StatusCode::OK => {
        // get mime from resp header
        let mime = {
          match resp.headers().get(header::CONTENT_TYPE) {
            Some(v) => match v.to_str() {
              Ok(v) => match v.parse::<Mime>() {
                Ok(v) => v,
                Err(e) => {
                  tracing::error!("failed to parse mime from header: {:?}", e);
                  mime::TEXT_PLAIN
                },
              },
              Err(e) => {
                tracing::error!("failed to get mime from header: {:?}", e);
                mime::TEXT_PLAIN
              },
            },
            None => mime::TEXT_PLAIN,
          }
        };

        let mut stream = resp.bytes_stream();
        let mut acc: Vec<u8> = Vec::new();
        while let Some(raw_bytes) = stream.next().await {
          acc.extend_from_slice(&raw_bytes?);
        }
        Ok((mime, acc))
      },
      reqwest::StatusCode::NOT_FOUND => Err(AppResponseError::from(AppError::RecordNotFound(
        url.to_owned(),
      ))),
      c => Err(AppResponseError::from(AppError::Unhandled(format!(
        "status code: {}, message: {}",
        c,
        resp.text().await?
      )))),
    }
  }

  pub async fn get_blob_metadata(&self, url: &str) -> Result<BlobMetadata, AppResponseError> {
    let resp = self
      .http_client_with_auth(Method::GET, url)
      .await?
      .send()
      .await?;

    log_request_id(&resp);
    AppResponse::<BlobMetadata>::from_response(resp)
      .await?
      .into_data()
  }

  pub async fn delete_blob(&self, url: &str) -> Result<(), AppResponseError> {
    let resp = self
      .http_client_with_auth(Method::DELETE, url)
      .await?
      .send()
      .await?;
    log_request_id(&resp);
    AppResponse::<()>::from_response(resp).await?.into_error()
  }

  pub async fn get_workspace_usage(
    &self,
    workspace_id: &str,
  ) -> Result<WorkspaceSpaceUsage, AppResponseError> {
    let url = format!("{}/api/file_storage/{}/usage", self.base_url, workspace_id);
    let resp = self
      .http_client_with_auth(Method::GET, &url)
      .await?
      .send()
      .await?;
    log_request_id(&resp);
    AppResponse::<WorkspaceSpaceUsage>::from_response(resp)
      .await?
      .into_data()
  }

  pub async fn get_workspace_all_blob_metadata(
    &self,
    workspace_id: &str,
  ) -> Result<RepeatedBlobMetaData, AppResponseError> {
    let url = format!("{}/api/file_storage/{}/blobs", self.base_url, workspace_id);
    let resp = self
      .http_client_with_auth(Method::GET, &url)
      .await?
      .send()
      .await?;
    log_request_id(&resp);
    AppResponse::<RepeatedBlobMetaData>::from_response(resp)
      .await?
      .into_data()
  }

  // Refresh token if given timestamp is close to the token expiration time
  pub async fn refresh_if_expired(&self, ts: i64) -> Result<(), AppResponseError> {
    let expires_at = self.token_expires_at()?;

    if ts + 30 > expires_at {
      // Add 30 seconds buffer
      self.refresh_token().await?;
    }
    Ok(())
  }

  #[instrument(level = "debug", skip_all, err)]
  pub async fn http_client_with_auth(
    &self,
    method: Method,
    url: &str,
  ) -> Result<RequestBuilder, AppResponseError> {
    let ts_now = chrono::Local::now().timestamp();
    self.refresh_if_expired(ts_now).await?;

    let access_token = self.access_token()?;
    trace!("start request: {}, method: {}", url, method);
    let request_builder = self
      .cloud_client
      .request(method, url)
      .header("client-version", CLIENT_API_VERSION.unwrap_or("unknown"))
      .header("client-timestamp", ts_now.to_string())
      .bearer_auth(access_token);
    Ok(request_builder)
  }

  #[instrument(level = "debug", skip_all, err)]
  pub(crate) async fn http_client_with_auth_compress(
    &self,
    method: Method,
    url: &str,
  ) -> Result<RequestBuilder, AppResponseError> {
    self
      .http_client_with_auth(method, url)
      .await
      .map(|builder| {
        builder
          .header(
            X_COMPRESSION_TYPE,
            HeaderValue::from_static(X_COMPRESSION_TYPE_BROTLI),
          )
          .header(
            X_COMPRESSION_BUFFER_SIZE,
            HeaderValue::from(self.config.compression_buffer_size),
          )
      })
  }

  pub(crate) fn batch_create_collab_url(&self, workspace_id: &str) -> String {
    format!(
      "{}/api/workspace/{}/batch/collab",
      self.base_url, workspace_id
    )
  }
}

impl Display for Client {
  fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
    f.write_fmt(format_args!(
      "Client {{ base_url: {}, ws_addr: {}, gotrue_url: {} }}",
      self.base_url, self.ws_addr, self.gotrue_client.base_url
    ))
  }
}

fn url_missing_param(param: &str) -> AppResponseError {
  AppError::InvalidRequest(format!("Url Missing Parameter:{}", param)).into()
}

pub(crate) fn log_request_id(resp: &reqwest::Response) {
  if let Some(request_id) = resp.headers().get("x-request-id") {
    event!(tracing::Level::DEBUG, "request_id: {:?}", request_id);
  } else {
    event!(tracing::Level::DEBUG, "request_id: not found");
  }
}

pub async fn spawn_blocking_brotli_compress(
  data: Vec<u8>,
  quality: u32,
  buffer_size: usize,
) -> Result<Vec<u8>, AppError> {
  tokio::task::spawn_blocking(move || {
    event!(
      tracing::Level::DEBUG,
      "start compressing collab with len:{}",
      data.len(),
    );
    let mut compressor = CompressorReader::new(&*data, buffer_size, quality, 22);
    let mut compressed_data = Vec::new();
    compressor
      .read_to_end(&mut compressed_data)
      .map_err(|err| AppError::InvalidRequest(format!("Failed to compress data: {}", err)))?;

    event!(
      tracing::Level::DEBUG,
      "compress collab success: before:{}, after:{}",
      data.len(),
      compressed_data.len()
    );
    Ok(compressed_data)
  })
  .await
  .map_err(AppError::from)?
}
