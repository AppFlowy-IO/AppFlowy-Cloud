use tokio::net::TcpListener;

use collab_history::app::create_app;
use tracing::info;

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

  let app = create_app().await.unwrap();
  axum::serve(listener, app)
    .await
    .expect("failed to run server");
}
