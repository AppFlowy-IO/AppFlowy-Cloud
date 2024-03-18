use crate::error::Error;
use crate::models::GetCollabHistoryRequest;
use crate::response::APIResponse;
use crate::AppState;
use axum::extract::State;
use axum::routing::post;
use axum::{Form, Router};

pub fn router() -> Router<AppState> {
  Router::new().route("/collab/history", post(get_collab_history))
}

pub async fn get_collab_history(
  State(_state): State<AppState>,
  Form(_param): Form<GetCollabHistoryRequest>,
) -> axum::response::Result<APIResponse<()>, Error> {
  Ok(APIResponse::new(()).with_message("hello".to_string()))
}
