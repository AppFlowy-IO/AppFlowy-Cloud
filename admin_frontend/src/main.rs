mod askama_entities;
mod config;
mod error;
mod ext;
mod models;
mod response;
mod session;
mod templates;
mod web_api;
mod web_app;

use axum::{response::Redirect, routing::get, Router};
use models::AppState;
use tokio::net::TcpListener;
use tower_http::services::ServeDir;
use tracing::info;

use crate::config::Config;

#[tokio::main]
async fn main() {
  // load from .env
  dotenvy::dotenv().ok();

  // set up tracing
  tracing_subscriber::fmt()
    .json()
    .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
    .with_line_number(true)
    .init();

  let config = Config::from_env().unwrap();
  info!("config loaded: {:?}", &config);

  let gotrue_client = gotrue::api::Client::new(reqwest::Client::new(), &config.gotrue_url);
  gotrue_client
    .health()
    .await
    .expect("gotrue health check failed");
  info!("Gotrue client initialized.");

  let redis_client = redis::Client::open(config.redis_url.clone())
    .expect("failed to create redis client")
    .get_connection_manager()
    .await
    .expect("failed to get redis connection manager");
  info!("Redis client initialized.");

  let session_store = session::SessionStorage::new(redis_client);

  let address = format!("{}:{}", config.host, config.port);
  let state = AppState {
    appflowy_cloud_url: config.appflowy_cloud_url.clone(),
    gotrue_client,
    session_store,
    config,
  };

  let web_app_router = web_app::router(state.clone()).with_state(state.clone());
  let web_api_router = web_api::router().with_state(state);

  let app = Router::new()
    .route(
      "/favicon.ico",
      get(|| async { Redirect::permanent("/assets/favicon.ico") }),
    )
    .route("/", get(|| async { Redirect::permanent("/web") }))
    .nest_service("/web", web_app_router)
    .nest_service("/web-api", web_api_router)
    .nest_service("/assets", ServeDir::new("assets"));

  let listener = TcpListener::bind(address)
    .await
    .expect("failed to bind to port");
  info!("listening on: {:?}", listener);
  axum::serve(listener, app)
    .await
    .expect("failed to run server");
}
