use axum::{routing::post, Router};

pub fn api_router() -> Router {
    Router::new().route("/test", post(test_handler))
}
pub async fn test_handler() -> String {
    "Test from api".to_string()
}
