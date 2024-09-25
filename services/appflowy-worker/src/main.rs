mod application;
mod config;
mod error;
mod notion_import;

use crate::application::run_server;
use crate::config::Config;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
  let listener = TcpListener::bind("0.0.0.0:4001").await.unwrap();
  let config = Config::from_env().expect("failed to load config");
  run_server(listener, config).await
}
