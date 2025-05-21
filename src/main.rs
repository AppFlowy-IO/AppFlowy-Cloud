use appflowy_cloud::application::{Application, init_state};
use appflowy_cloud::config::config::get_configuration;
use appflowy_cloud::telemetry::init_subscriber;

#[actix_web::main]
async fn main() -> anyhow::Result<()> {
  let level = std::env::var("RUST_LOG").unwrap_or("info".to_string());
  println!("AppFlowy Cloud with RUST_LOG={}", level);

  // Load environment variables from .env file
  dotenvy::dotenv().ok();

  let conf =
    get_configuration().map_err(|e| anyhow::anyhow!("Failed to read configuration: {}", e))?;

  init_subscriber(&conf.app_env);

  let (tx, rx) = tokio::sync::mpsc::channel(1000);
  let state = init_state(&conf, tx)
    .await
    .map_err(|e| anyhow::anyhow!("Failed to initialize application state: {}", e))?;
  let application = Application::build(conf, state, rx).await?;
  application.run_until_stopped().await?;

  Ok(())
}
