use crate::biz::user::user_info::{get_profile, get_user_workspace_info, update_user};
use crate::biz::user::user_verify::verify_token;
use crate::state::AppState;
use actix_web::web::{Data, Json};
use actix_web::Result;
use actix_web::{web, Scope};
use app_error::ErrorCode;
use authentication::jwt::{Authorization, UserUuid};
use database_entity::dto::{AFUserProfile, AFUserWorkspaceInfo};
use gotrue::params::AdminDeleteUserParams;
use secrecy::{ExposeSecret, Secret};
use shared_entity::dto::auth_dto::{DeleteUserQuery, SignInTokenResponse, UpdateUserParams};
use shared_entity::response::AppResponseError;
use shared_entity::response::{AppResponse, JsonAppResponse};

pub fn user_scope() -> Scope {
  web::scope("/api/user")
    .service(web::resource("/verify/{access_token}").route(web::get().to(verify_user_handler)))
    .service(web::resource("/update").route(web::post().to(update_user_handler)))
    .service(web::resource("/profile").route(web::get().to(get_user_profile_handler)))
    .service(web::resource("/workspace").route(web::get().to(get_user_workspace_info_handler)))
    .service(web::resource("").route(web::delete().to(delete_user_handler)))
}

#[tracing::instrument(skip(state, path), err)]
async fn verify_user_handler(
  path: web::Path<String>,
  state: Data<AppState>,
) -> Result<JsonAppResponse<SignInTokenResponse>> {
  let access_token = path.into_inner();
  let is_new = verify_token(&access_token, state.as_ref())
    .await
    .map_err(AppResponseError::from)?;
  let resp = SignInTokenResponse { is_new };
  Ok(AppResponse::Ok().with_data(resp).into())
}

#[tracing::instrument(skip(state), err)]
async fn get_user_profile_handler(
  uuid: UserUuid,
  state: Data<AppState>,
) -> Result<JsonAppResponse<AFUserProfile>> {
  let profile = get_profile(&state.pg_pool, &uuid)
    .await
    .map_err(AppResponseError::from)?;
  Ok(AppResponse::Ok().with_data(profile).into())
}

#[tracing::instrument(skip(state), err)]
async fn get_user_workspace_info_handler(
  uuid: UserUuid,
  state: Data<AppState>,
) -> Result<JsonAppResponse<AFUserWorkspaceInfo>> {
  let info = get_user_workspace_info(&state.pg_pool, &uuid).await?;
  Ok(AppResponse::Ok().with_data(info).into())
}

#[tracing::instrument(skip(state, auth, payload), err)]
async fn update_user_handler(
  auth: Authorization,
  payload: Json<UpdateUserParams>,
  state: Data<AppState>,
) -> Result<JsonAppResponse<()>> {
  let params = payload.into_inner();
  update_user(&state.pg_pool, auth.uuid()?, params).await?;
  Ok(AppResponse::Ok().into())
}

#[tracing::instrument(skip(state), err)]
async fn delete_user_handler(
  auth: Authorization,
  state: Data<AppState>,
  query: web::Query<DeleteUserQuery>,
) -> Result<JsonAppResponse<()>, actix_web::Error> {
  let user_uuid = auth.uuid()?;
  if is_apple_user(&auth) {
    let query = query.into_inner();
    if let Err(err) = revoke_apple_user(
      &state.config.apple_oauth.client_id,
      &state.config.apple_oauth.client_secret,
      query.provider_access_token,
      query.provider_refresh_token,
    )
    .await
    {
      tracing::warn!("revoke apple user failed: {:?}", err);
    };
  }

  let admin_token = state.gotrue_admin.token().await?;
  let _ = &state
    .gotrue_client
    .admin_delete_user(
      &admin_token,
      &user_uuid.to_string(),
      &AdminDeleteUserParams {
        should_soft_delete: false,
      },
    )
    .await
    .map_err(AppResponseError::from)?;

  Ok(AppResponse::Ok().into())
}

async fn revoke_apple_user(
  client_id: &str,
  client_secret: &Secret<String>,
  apple_access_token: Option<String>,
  apple_refresh_token: Option<String>,
) -> Result<(), AppResponseError> {
  let (type_type_hint, token) = match apple_access_token {
    Some(access_token) => ("access_token", access_token),
    None => match apple_refresh_token {
      Some(refresh_token) => ("refresh_token", refresh_token),
      None => {
        return Err(AppResponseError::new(
          ErrorCode::InvalidRequest,
          "apple email deletion must provide access_token or refresh_token",
        ))
      },
    },
  };

  if let Err(err) = revoke_apple_token_http_call(
    client_id,
    client_secret.expose_secret(),
    &token,
    type_type_hint,
  )
  .await
  {
    tracing::warn!("revoke apple token failed: {:?}", err);
  };
  Ok(())
}

fn is_apple_user(auth: &Authorization) -> bool {
  if let Some(provider) = auth.claims.app_metadata.get("provider") {
    if provider == "apple" {
      return true;
    }
  };

  if let Some(providers) = auth.claims.app_metadata.get("providers") {
    if let Some(providers) = providers.as_array() {
      for provider in providers {
        if provider == "apple" {
          return true;
        }
      }
    }
  }

  false
}

/// Based on: https://developer.apple.com/documentation/sign_in_with_apple/revoke_tokens
async fn revoke_apple_token_http_call(
  apple_client_id: &str,
  apple_client_secret: &str,
  apple_user_token: &str,
  token_type_hint: &str,
) -> Result<(), AppResponseError> {
  let resp = reqwest::Client::new()
    .post("https://appleid.apple.com/auth/revoke")
    .form(&[
      ("client_id", apple_client_id),
      ("client_secret", apple_client_secret),
      ("token", apple_user_token),
      ("token_type_hint", token_type_hint),
    ])
    .send()
    .await?;

  let status = resp.status();
  if status.is_success() {
    return Ok(());
  }

  let payload = resp.text().await?;
  Err(AppResponseError::new(
    ErrorCode::AppleRevokeTokenError,
    format!(
      "calling apple revoke, code: {}, message: {}",
      status, payload
    ),
  ))
}
