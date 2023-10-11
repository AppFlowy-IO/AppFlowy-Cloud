mod access_token;
mod error;
mod models;
mod response;
mod session;
mod templates;
mod web_api;
mod web_app;

use axum::{response::Redirect, routing::get, Router};
use reqwest::Method;
use tower::ServiceBuilder;
use tower_http::cors::{Any, CorsLayer};

#[tokio::main]
async fn main() {
  // load from .env
  dotenv::dotenv().ok();

  let gotrue_client = gotrue::api::Client::new(
    reqwest::Client::new(),
    &std::env::var("GOTRUE_URL").unwrap_or("http://gotrue:9999".to_string()),
  );
  let redis_client =
    redis::Client::open(std::env::var("REDIS_URL").unwrap_or("redis://redis:6379".to_string()))
      .unwrap()
      .get_tokio_connection_manager()
      .await
      .unwrap();
  let session_store = session::SessionStorage::new(redis_client);

  let state = AppState {
    gotrue_client,
    session_store,
  };

  let web_app_router = web_app::router().with_state(state.clone());
  let web_api_router = web_api::router().with_state(state);

  let cors = CorsLayer::new()
    // allow `GET` and `POST` when accessing the resource
    .allow_methods([Method::GET, Method::POST])
    // allow requests from any origin
    .allow_origin(Any);

  let app = Router::new()
    .route("/", get(|| async { Redirect::permanent("/web") }))
    .layer(ServiceBuilder::new().layer(cors))
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
  pub session_store: session::SessionStorage,
}
