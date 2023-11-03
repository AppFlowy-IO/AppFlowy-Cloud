use appflowy_cloud::application::{init_state, Application};
use appflowy_cloud::config::config::{get_configuration, Environment};
use appflowy_cloud::telemetry::init_subscriber;

#[actix_web::main]
async fn main() -> anyhow::Result<()> {
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

  let app_env: Environment = std::env::var("APP_ENVIRONMENT")
    .unwrap_or_else(|_| "local".to_string())
    .try_into()
    .expect("Failed to parse APP_ENVIRONMENT.");

  init_subscriber(&app_env, filters);
  let configuration = get_configuration(&app_env).expect("The configuration should be configured.");
  let state = init_state(&configuration)
    .await
    .expect("The AppState should be initialized");
  let application = Application::build(configuration, state).await?;
  application.run_until_stopped().await?;

  Ok(())
}
