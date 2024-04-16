use appflowy_history::application::run_server;
use appflowy_history::config::Config;

use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
  let listener = TcpListener::bind("0.0.0.0:50051").await.unwrap();
  let config = Config::from_env().expect("failed to load config");
  run_server(listener, config).await
}
