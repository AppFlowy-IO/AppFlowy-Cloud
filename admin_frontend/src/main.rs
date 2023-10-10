mod error;
mod models;
mod response;
mod templates;
mod web_api;
mod web_app;

use axum::{response::Redirect, routing::get, Router};

#[tokio::main]
async fn main() {
  // load from .env
  dotenv::dotenv().ok();

  let gotrue_client = gotrue::api::Client::new(
    reqwest::Client::new(),
    &std::env::var("GOTRUE_URL").unwrap_or("http://gotrue:9999".to_string()),
  );

  let state = AppState { gotrue_client };

  let web_app_router = web_app::router();
  let web_api_router = web_api::router().with_state(state);

  let app = Router::new()
    .route("/", get(|| async { Redirect::permanent("/web") }))
    .nest_service("/web", web_app_router)
    .nest_service("/web-api", web_api_router);

  axum::Server::bind(&"0.0.0.0:3000".parse().unwrap())
    .serve(app.into_make_service())
    .await
    .unwrap();
}

#[derive(Clone)]
pub struct AppState {
  pub gotrue_client: gotrue::api::Client,
}
