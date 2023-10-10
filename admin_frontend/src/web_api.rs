use crate::error::WebApiError;
use crate::models::LoginResponse;
use crate::response::WebApiResponse;
use crate::{models::LoginRequest, AppState};
use axum::response::Result;
use axum::Json;
use axum::{extract::State, routing::post, Router};

pub fn router() -> Router<AppState> {
  Router::new()
      // TODO
    .route("/login", post(login_handler))
}

// TODO: Support OAuth2 login
// login and set the cookie
pub async fn login_handler(
  State(state): State<AppState>,
  Json(param): Json<LoginRequest>,
) -> Result<WebApiResponse<LoginResponse>, WebApiError> {
  let token = state
    .gotrue_client
    .token(&gotrue::grant::Grant::Password(
      gotrue::grant::PasswordGrant {
        email: param.email.to_owned(),
        password: param.password.to_owned(),
      },
    ))
    .await?;

  Ok(WebApiResponse::new(LoginResponse {
    access_token: token.access_token,
  }))
}
