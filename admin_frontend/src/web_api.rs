use crate::error::WebApiError;
use crate::ext::api::{
  accept_workspace_invitation, delete_current_user, invite_user_to_workspace, leave_workspace,
  verify_token_cloud,
};
use crate::models::{AppState, WebApiLoginRequest};
use crate::models::{
  LoginParams, OAuthRedirect, OAuthRedirectToken, WebApiAdminCreateUserRequest,
  WebApiChangePasswordRequest, WebApiCreateSSOProviderRequest, WebApiInviteUserRequest,
  WebApiPutUserRequest,
};
use crate::response::WebApiResponse;
use crate::session::{self, new_session_cookie, CodeSession, UserSession};
use axum::extract::{Path, Query};
use axum::http::{status, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Result};
use axum::routing::{delete, get};
use axum::Form;
use axum::{extract::State, routing::post, Router};
use axum_extra::extract::cookie::Cookie;
use axum_extra::extract::CookieJar;
use base64::engine::Engine;
use base64::prelude::BASE64_STANDARD_NO_PAD;
use gotrue::params::{
  AdminDeleteUserParams, AdminUserParams, CreateSSOProviderParams, GenerateLinkParams,
  MagicLinkParams,
};
use gotrue_entity::dto::{GotrueTokenResponse, SignUpResponse, UpdateGotrueUserParams, User};
use rand::distributions::Alphanumeric;
use rand::Rng;
use sha2::Digest;
use tracing::info;

pub fn router() -> Router<AppState> {
  Router::new()
    .route("/signin", post(sign_in_handler))
    .route("/oauth-redirect", get(oauth_redirect_handler))
    .route("/oauth-redirect/token", get(oauth_redirect_token_handler))
    .route("/signup", post(sign_up_handler))
    .route("/login-refresh/:refresh_token", post(login_refresh_handler))
    .route("/logout", post(logout_handler))

    // user
    .route("/change-password", post(change_password_handler))
    .route("/oauth_login/:provider", post(post_oauth_login_handler))
    .route("/invite", post(invite_handler))
    .route("/workspace/:workspace_id/invite", post(workspace_invite_handler))
    .route("/workspace/:workspace_id/leave", post(leave_workspace_handler))
    .route("/invite/:invite_id/accept", post(invite_accept_handler))
    .route("/open_app", post(open_app_handler))
    .route("/delete-account", delete(delete_account_handler))

    // admin
    .route("/admin/user", post(admin_add_user_handler))
    .route(
      "/admin/user/:user_uuid",
      delete(admin_delete_user_handler).put(admin_update_user_handler),
    )
    .route(
      "/admin/user/:email/generate-link",
      post(post_user_generate_link_handler),
    )
    .route("/admin/sso", post(admin_create_sso_handler))
    .route("/admin/sso/:provider_id", delete(admin_delete_sso_handler))
}

async fn admin_delete_sso_handler(
  State(state): State<AppState>,
  session: UserSession,
  Path(provider_id): Path<String>,
) -> Result<WebApiResponse<()>, WebApiError<'static>> {
  let _ = state
    .gotrue_client
    .admin_delete_sso_provider(&session.token.access_token, &provider_id)
    .await?;

  Ok(WebApiResponse::<()>::from_str("SSO Deleted".into()))
}

async fn admin_create_sso_handler(
  State(state): State<AppState>,
  session: UserSession,
  Form(param): Form<WebApiCreateSSOProviderRequest>,
) -> Result<WebApiResponse<()>, WebApiError<'static>> {
  let provider_params = CreateSSOProviderParams {
    type_: param.type_,
    metadata_url: param.metadata_url,
    ..Default::default()
  };

  let _ = state
    .gotrue_client
    .admin_create_sso_providers(&session.token.access_token, &provider_params)
    .await?;

  Ok(WebApiResponse::<()>::from_str("SSO Added".into()))
}

/// Generates a URL to facilitate login redirection to the AppFlowy app from a web browser.
///
/// This function creates a custom URL scheme that can be used in a web browser to open the
/// AppFlowy app and automatically handle user login based on the provided `UserSession`.
///
/// # Returns
/// A `Result` containing `HeaderMap` for HTTP redirection if successful, or `WebApiError` in case of failure.
///
/// # Example URL Format
/// `appflowy-flutter://login-callback#access_token=...&expires_at=...&expires_in=...&refresh_token=...&token_type=...`
///
/// The URL includes access token information and other relevant session details.
///
/// # Usage
/// The client application should implement handling for this URL format, typically through the
/// `sign_in_with_url` method in the `client-api` crate. See [client_api::Client::sign_in_with_url] for more details.
///
async fn open_app_handler(
  session: UserSession,
) -> Result<axum::response::Response, WebApiError<'static>> {
  let app_sign_in_url = format!(
      "appflowy-flutter://login-callback#access_token={}&expires_at={}&expires_in={}&refresh_token={}&token_type={}",
        session.token.access_token,
        session.token.expires_at,
        session.token.expires_in,
        session.token.refresh_token,
        session.token.token_type,
  );
  Ok(htmx_redirect(&app_sign_in_url).into_response())
}

/// Delete the user account and all associated data.
async fn delete_account_handler(
  state: State<AppState>,
  session: UserSession,
) -> Result<axum::response::Response, WebApiError<'static>> {
  delete_current_user(&session.token.access_token, &state.appflowy_cloud_url).await?;
  let redirect_url = state.prepend_with_path_prefix("/web/login");
  Ok(Redirect::to(&redirect_url).into_response())
}

// Invite another user, this will trigger email sending
// to the target user
async fn invite_handler(
  State(state): State<AppState>,
  Form(param): Form<WebApiInviteUserRequest>,
) -> Result<WebApiResponse<()>, WebApiError<'static>> {
  let magic_link_redirect = if state.config.path_prefix.is_empty() {
    "/".to_owned()
  } else {
    state.config.path_prefix.clone()
  };
  state
    .gotrue_client
    .magic_link(
      &MagicLinkParams {
        email: param.email,
        ..Default::default()
      },
      Some(magic_link_redirect),
    )
    .await?;
  Ok(WebApiResponse::<()>::from_str("Invitation sent".into()))
}

async fn workspace_invite_handler(
  State(state): State<AppState>,
  session: UserSession,
  Path(workspace_id): Path<String>,
  Form(param): Form<WebApiInviteUserRequest>,
) -> Result<WebApiResponse<()>, WebApiError<'static>> {
  invite_user_to_workspace(
    &session.token.access_token,
    &workspace_id,
    &param.email,
    &state.appflowy_cloud_url,
  )
  .await?;

  Ok(WebApiResponse::<()>::from_str("Invitation sent".into()))
}

async fn leave_workspace_handler(
  State(state): State<AppState>,
  session: UserSession,
  Path(workspace_id): Path<String>,
) -> Result<WebApiResponse<()>, WebApiError<'static>> {
  leave_workspace(
    &session.token.access_token,
    &workspace_id,
    &state.appflowy_cloud_url,
  )
  .await?;

  Ok(WebApiResponse::<()>::from_str("Left workspace".into()))
}

async fn invite_accept_handler(
  State(state): State<AppState>,
  session: UserSession,
  Path(invite_id): Path<String>,
) -> Result<HeaderMap, WebApiError<'static>> {
  accept_workspace_invitation(
    &session.token.access_token,
    &invite_id,
    &state.appflowy_cloud_url,
  )
  .await?;

  Ok(htmx_trigger("workspaceInvitationAccepted"))
}

async fn change_password_handler(
  State(state): State<AppState>,
  session: UserSession,
  Form(param): Form<WebApiChangePasswordRequest>,
) -> Result<WebApiResponse<()>, WebApiError<'static>> {
  if param.new_password != param.confirm_password {
    return Err(WebApiError::new(
      status::StatusCode::BAD_REQUEST,
      "passwords do not match",
    ));
  }
  let _user = state
    .gotrue_client
    .update_user(
      &session.token.access_token,
      &UpdateGotrueUserParams {
        password: Some(param.new_password),
        ..Default::default()
      },
    )
    .await?;
  Ok(WebApiResponse::<()>::from_str("Password changed".into()))
}

async fn post_oauth_login_handler(
  header_map: HeaderMap,
  Path(provider): Path<String>,
) -> Result<WebApiResponse<String>, WebApiError<'static>> {
  let base_url = get_base_url(&header_map);
  let redirect_uri = format!("{}/web/oauth_login_redirect", base_url);

  let oauth_url = format!(
    "{}/authorize?provider={}&redirect_uri={}",
    base_url, &provider, redirect_uri
  );
  Ok(oauth_url.into())
}

async fn admin_update_user_handler(
  State(state): State<AppState>,
  session: UserSession,
  Path(user_uuid): Path<String>,
  Form(param): Form<WebApiPutUserRequest>,
) -> Result<WebApiResponse<User>, WebApiError<'static>> {
  let res = state
    .gotrue_client
    .admin_update_user(
      &session.token.access_token,
      &user_uuid,
      &AdminUserParams {
        password: Some(param.password.to_owned()),
        email_confirm: true,
        ..Default::default()
      },
    )
    .await?;
  Ok(res.into())
}

async fn post_user_generate_link_handler(
  State(state): State<AppState>,
  session: UserSession,
  Path(email): Path<String>,
) -> Result<String, WebApiError<'static>> {
  let res = state
    .gotrue_client
    .admin_generate_link(
      &session.token.access_token,
      &GenerateLinkParams {
        email,
        ..Default::default()
      },
    )
    .await?;
  Ok(res.action_link)
}

async fn admin_delete_user_handler(
  State(state): State<AppState>,
  session: UserSession,
  Path(user_uuid): Path<String>,
) -> Result<WebApiResponse<()>, WebApiError<'static>> {
  state
    .gotrue_client
    .admin_delete_user(
      &session.token.access_token,
      &user_uuid,
      &AdminDeleteUserParams {
        should_soft_delete: false,
      },
    )
    .await?;
  Ok(().into())
}

async fn admin_add_user_handler(
  State(state): State<AppState>,
  session: UserSession,
  Form(param): Form<WebApiAdminCreateUserRequest>,
) -> Result<WebApiResponse<()>, WebApiError<'static>> {
  let add_user_params = AdminUserParams {
    email: param.email,
    password: Some(param.password),
    email_confirm: !param.require_email_verification,
    ..Default::default()
  };
  let _user = state
    .gotrue_client
    .admin_add_user(&session.token.access_token, &add_user_params)
    .await?;
  Ok(WebApiResponse::<()>::from_str("User created".into()))
}

async fn login_refresh_handler(
  State(state): State<AppState>,
  jar: CookieJar,
  Path(refresh_token): Path<String>,
  Query(login): Query<LoginParams>,
) -> Result<axum::response::Response, WebApiError<'static>> {
  let token = state
    .gotrue_client
    .token(&gotrue::grant::Grant::RefreshToken(
      gotrue::grant::RefreshTokenGrant { refresh_token },
    ))
    .await?;

  // Do another round of refresh_token to consume and invalidate the old one
  let token = state
    .gotrue_client
    .token(&gotrue::grant::Grant::RefreshToken(
      gotrue::grant::RefreshTokenGrant {
        refresh_token: token.refresh_token,
      },
    ))
    .await?;

  session_login(State(state), token, jar, login.redirect_to.as_deref()).await
}

// login and set the cookie
// sign up if not exist
async fn sign_in_handler(
  State(state): State<AppState>,
  jar: CookieJar,
  Form(param): Form<WebApiLoginRequest>,
) -> Result<axum::response::Response, WebApiError<'static>> {
  let WebApiLoginRequest {
    email,
    password,
    redirect_to,
  } = param;

  if password.is_empty() {
    let res = send_magic_link(State(state), &email).await?;
    return Ok(res.into_response());
  }

  // Attempt to sign in with email and password
  let token = state
    .gotrue_client
    .token(&gotrue::grant::Grant::Password(
      gotrue::grant::PasswordGrant {
        email: email.to_owned(),
        password: password.to_owned(),
      },
    ))
    .await?;

  session_login(State(state), token, jar, redirect_to.as_deref()).await
}

async fn oauth_redirect_handler(
  State(state): State<AppState>,
  session: UserSession,
  Query(oauth_redirect): Query<OAuthRedirect>,
) -> Result<axum::response::Response, WebApiError<'static>> {
  {
    // OAuthRedirect verification
    if oauth_redirect.client_id != state.config.oauth.client_id {
      return Err(WebApiError::new(
        StatusCode::BAD_REQUEST,
        "invalid client_id",
      ));
    }
    if oauth_redirect.response_type != "code" {
      return Err(WebApiError::new(
        StatusCode::BAD_REQUEST,
        "invalid response_type, only 'code' is support",
      ));
    }
    {
      // Check if the redirect_uri is in the allowable list
      let mut found = false;
      for allowable_uri in &state.config.oauth.allowable_redirect_uris {
        if oauth_redirect.redirect_uri == *allowable_uri {
          found = true;
          break;
        }
      }
      if !found {
        return Err(WebApiError::new(
          StatusCode::BAD_REQUEST,
          format!(
            "invalid redirect_uri: {}, allowable_uris: {}",
            oauth_redirect.redirect_uri,
            state.config.oauth.allowable_redirect_uris.join(", ")
          ),
        ));
      }
    }
  }

  let code = gen_rand_alpha_num(32);
  state
    .session_store
    .put_code_session(
      &code,
      &CodeSession {
        session_id: session.session_id.clone(),
        code_challenge: oauth_redirect.code_challenge,
        code_challenge_method: oauth_redirect.code_challenge_method,
      },
    )
    .await?;

  let url = format!(
    "{}?code={}&state={}",
    oauth_redirect.redirect_uri, code, oauth_redirect.state,
  );
  let resp = Redirect::to(&url).into_response();
  Ok(resp)
}

async fn oauth_redirect_token_handler(
  State(state): State<AppState>,
  Query(token_req): Query<OAuthRedirectToken>,
) -> Result<axum::response::Response, WebApiError<'static>> {
  // Check client secret (if exists)
  if let Some(server_client_secret) = state.config.oauth.client_secret {
    match token_req.client_secret {
      Some(given_client_secret) => {
        if server_client_secret != given_client_secret {
          return Err(WebApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid client_secret",
          ));
        }
      },
      _ => {
        return Err(WebApiError::new(
          StatusCode::BAD_REQUEST,
          "expecting client_secret",
        ));
      },
    }
  };

  let code_session = state
    .session_store
    .get_code_session(&token_req.code)
    .await?
    .ok_or_else(|| WebApiError::new(StatusCode::BAD_REQUEST, "invalid code"))?;

  if let Some(code_challenge) = code_session.code_challenge {
    match code_session.code_challenge_method.as_deref() {
      Some("S256") => {
        let verifier = token_req.code_verifier.ok_or_else(|| {
          WebApiError::new(status::StatusCode::BAD_REQUEST, "missing code_verifier")
        })?;

        // get code challenge based64 decoded
        let code_challenge = BASE64_STANDARD_NO_PAD
          .decode(code_challenge)
          .map_err(|err| {
            WebApiError::new(
              status::StatusCode::BAD_REQUEST,
              format!("failed to base64 decode code challege: {}", err),
            )
          })?;

        // hash the verifier and check against the original code challenge
        let mut hasher = sha2::Sha256::new();
        hasher.update(verifier.as_bytes());
        let verifier_hashed = hasher.finalize().to_vec();
        if verifier_hashed != code_challenge {
          return Err(WebApiError::new(
            status::StatusCode::BAD_REQUEST,
            "invalid code_verifier",
          ));
        }
      },
      _ => {
        return Err(WebApiError::new(
          status::StatusCode::BAD_REQUEST,
          "invalid code_challenge_method, only support S256",
        ));
      },
    }
  }

  let user_session = state
    .session_store
    .get_user_session(&code_session.session_id)
    .await?
    .ok_or_else(|| WebApiError::new(StatusCode::BAD_REQUEST, "invalid session"))?;

  let resp = axum::Json::from(user_session.token);
  Ok(resp.into_response())
}

async fn sign_up_handler(
  State(state): State<AppState>,
  jar: CookieJar,
  Form(param): Form<WebApiLoginRequest>,
) -> Result<axum::response::Response, WebApiError<'static>> {
  let WebApiLoginRequest {
    email,
    password,
    redirect_to,
  } = param;

  if password.is_empty() {
    let res = send_magic_link(State(state), &email).await?;
    return Ok(res.into_response());
  }

  let sign_up_res = state
    .gotrue_client
    .sign_up(&email, &password, Some("/"))
    .await?;

  match sign_up_res {
    // when GOTRUE_MAILER_AUTOCONFIRM=true, auto sign in
    SignUpResponse::Authenticated(token) => {
      session_login(State(state), token, jar, redirect_to.as_deref()).await
    },
    SignUpResponse::NotAuthenticated(user) => {
      info!("user signed up and not authenticated: {:?}", user);
      Ok(WebApiResponse::<()>::from_str("Email Verification Sent".into()).into_response())
    },
  }
}

async fn logout_handler(
  State(state): State<AppState>,
  jar: CookieJar,
) -> Result<axum::response::Response, WebApiError<'static>> {
  let session_id = jar
    .get("session_id")
    .ok_or(WebApiError::new(
      status::StatusCode::BAD_REQUEST,
      "no session_id cookie",
    ))?
    .value();

  state.session_store.del_user_session(session_id).await?;
  let htmx_redirect_url = format!("{}/web/login", state.config.path_prefix);
  Ok(
    (
      jar.remove(Cookie::from("session_id")),
      htmx_redirect(&htmx_redirect_url),
    )
      .into_response(),
  )
}

fn htmx_trigger(trigger: &str) -> HeaderMap {
  let mut h = HeaderMap::new();
  h.insert("HX-Trigger", trigger.parse().unwrap());
  h
}

async fn session_login(
  State(state): State<AppState>,
  token: GotrueTokenResponse,
  jar: CookieJar,
  redirect_to: Option<&str>,
) -> Result<axum::response::Response, WebApiError<'static>> {
  verify_token_cloud(
    token.access_token.as_str(),
    state.appflowy_cloud_url.as_str(),
  )
  .await?;

  let new_session_id = uuid::Uuid::new_v4();
  let new_session = session::UserSession {
    session_id: new_session_id.to_string(),
    token,
  };
  state.session_store.put_user_session(&new_session).await?;

  let decoded_redirect_to = redirect_to.and_then(|s| match urlencoding::decode(s) {
    Ok(r) => Some(r),
    Err(err) => {
      tracing::error!("failed to decode redirect_to: {}", err);
      None
    },
  });
  let default_htmx_redirect_url = format!("{}/web/home", state.config.path_prefix);
  Ok(
    (
      jar.add(new_session_cookie(new_session_id)),
      htmx_redirect(
        decoded_redirect_to
          .as_deref()
          .unwrap_or(&default_htmx_redirect_url),
      ),
    )
      .into_response(),
  )
}

async fn send_magic_link(
  State(state): State<AppState>,
  email: &str,
) -> Result<WebApiResponse<()>, WebApiError<'static>> {
  state
    .gotrue_client
    .magic_link(
      &MagicLinkParams {
        email: email.to_owned(),
        ..Default::default()
      },
      Some(format!("{}/web/login-callback", state.config.path_prefix)),
    )
    .await?;
  Ok(WebApiResponse::<()>::from_str("Magic Link Sent".into()))
}

fn htmx_redirect(url: &str) -> HeaderMap {
  let mut h = HeaderMap::new();
  h.insert("Location", url.parse().unwrap());
  h.insert("HX-Redirect", url.parse().unwrap());
  h
}

fn get_base_url(header_map: &HeaderMap) -> String {
  let scheme = get_header_value_or_default(header_map, "x-scheme", "http");
  let host = get_header_value_or_default(header_map, "host", "localhost");

  format!("{}://{}", scheme, host)
}

fn get_header_value_or_default<'a>(
  header_map: &'a HeaderMap,
  header_name: &str,
  default: &'a str,
) -> &'a str {
  match header_map.get(header_name) {
    Some(v) => match v.to_str() {
      Ok(v) => v,
      Err(e) => {
        tracing::error!("failed to get header value {}: {}, {:?}", header_name, e, v);
        default
      },
    },
    None => default,
  }
}

fn gen_rand_alpha_num(n: usize) -> String {
  let random_string: String = rand::thread_rng()
    .sample_iter(&Alphanumeric)
    .take(n)
    .map(char::from)
    .collect();
  random_string
}
