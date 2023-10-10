use axum::{
  async_trait,
  extract::FromRequestParts,
  http::request::Parts,
  response::{IntoResponse, Redirect},
};
use axum_extra::extract::CookieJar;

pub struct WebAccessToken(pub String);

#[async_trait]
impl<S> FromRequestParts<S> for WebAccessToken
where
  S: Send + Sync,
{
  type Rejection = AccessTokenRejection;

  async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
    let jar = CookieJar::from_request_parts(parts, state)
      .await
      .map_err(|e| AccessTokenRejection::CookieError(e.to_string()))?;

    let token = jar
      .get("access_token")
      .ok_or(AccessTokenRejection::NoAccessToken)?
      .to_string();

    Ok(WebAccessToken(token))
  }
}

#[derive(Clone, Debug)]
pub enum AccessTokenRejection {
  NoAccessToken,
  CookieError(String),
}

impl IntoResponse for AccessTokenRejection {
  fn into_response(self) -> axum::response::Response {
    match self {
      AccessTokenRejection::NoAccessToken => Redirect::permanent("/web/login").into_response(),
      AccessTokenRejection::CookieError(err) => {
        println!("cookie error: {}", err);
        Redirect::permanent("/web/login").into_response()
      },
    }
  }
}
