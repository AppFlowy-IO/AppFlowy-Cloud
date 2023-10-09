use axum::{routing::post, Router};
use leptos::{server, ServerFnError};

pub fn api_router() -> Router {
  Router::new().route("/test", post(test_handler))
}

pub async fn test_handler() -> String {
  "Test from api".to_string()
}

#[server(Foo)]
pub async fn foo() -> Result<(), ServerFnError> {
  println!("Foo!");
  Ok(())
}
