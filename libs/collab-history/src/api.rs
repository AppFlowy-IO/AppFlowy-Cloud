use crate::error::Error;
use crate::response::APIResponse;
use crate::AppState;
use axum::extract::State;
use axum::routing::get;
use axum::Router;

pub fn router() -> Router<AppState> {
  Router::new().route("/hello", get(hello_handler))
}

pub async fn hello_handler(
  State(_state): State<AppState>,
) -> axum::response::Result<APIResponse<()>, Error> {
  Ok(APIResponse::new(()).with_message("hello".to_string()))
}
