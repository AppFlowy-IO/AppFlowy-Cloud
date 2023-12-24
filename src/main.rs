use appflowy_cloud::application::{init_state, Application};
use appflowy_cloud::config::config::get_configuration;
use appflowy_cloud::telemetry::init_subscriber;

#[actix_web::main]
async fn main() -> anyhow::Result<()> {
  // load from .env
  dotenvy::dotenv().ok();

  let level = std::env::var("RUST_LOG").unwrap_or("info".to_string());
  println!("AppFlowy Cloud with RUST_LOG={}", level);

  let mut filters = vec![];
  filters.push(format!("actix_web={}", level));
  filters.push(format!("collab={}", level));
  filters.push(format!("collab_sync={}", level));
  filters.push(format!("appflowy_cloud={}", level));
  filters.push(format!("collab_plugins={}", level));
  filters.push(format!("realtime={}", level));
  filters.push(format!("database={}", level));
  filters.push(format!("storage={}", level));

  let conf =
    get_configuration().map_err(|e| anyhow::anyhow!("Failed to read configuration: {}", e))?;

  init_subscriber(&conf.app_env, filters);

  let state = init_state(&conf)
    .await
    .map_err(|e| anyhow::anyhow!("Failed to initialize application state: {}", e))?;
  let application = Application::build(conf, state).await?;
  application.run_until_stopped().await?;

  Ok(())
}
