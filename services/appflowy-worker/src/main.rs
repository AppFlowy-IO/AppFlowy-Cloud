mod application;
mod config;
pub mod error;
pub mod import_worker;
pub(crate) mod s3_client;

mod metric;

mod mailer;
use crate::application::run_server;
use crate::config::Config;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
  dotenvy::dotenv().ok();

  let listener = TcpListener::bind("[::]:4001").await.unwrap();
  let config = Config::from_env()?;
  run_server(listener, config).await
}
