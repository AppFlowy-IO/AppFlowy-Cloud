use appflowy_history::application::run_server;
use appflowy_history::config::Config;
use std::net::SocketAddr;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
  let addr = "[::1]:50051".parse::<SocketAddr>()?;
  let listener = TcpListener::bind(addr).await?;
  let config = Config::from_env().expect("failed to load config");
  run_server(listener, config).await
}
