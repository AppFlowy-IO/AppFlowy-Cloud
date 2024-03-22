use crate::app::create_app;

use redis::aio::ConnectionManager;
use tokio::net::TcpListener;

use tracing::info;

mod api;
mod app;
mod config;
mod error;
mod models;
mod response;

#[tokio::main]
async fn main() {
  dotenvy::dotenv().ok();
  tracing_subscriber::fmt()
    .json()
    .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
    .init();

  let listener = TcpListener::bind("0.0.0.0:3100")
    .await
    .expect("failed to bind to port");
  info!("listening on: {:?}", listener);
  axum::serve(listener, create_app().await)
    .await
    .expect("failed to run server");
}

#[derive(Clone)]
pub struct AppState {
  pub redis_client: ConnectionManager,
}
