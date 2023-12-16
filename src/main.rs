use appflowy_cloud::application::{init_state, Application};
use appflowy_cloud::config::config::{get_configuration, Environment};
use appflowy_cloud::telemetry::init_subscriber;
use std::panic;
use tracing::error;

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

  // Set panic hook
  panic::set_hook(Box::new(|panic_info| {
    let panic_message = if let Some(s) = panic_info.payload().downcast_ref::<&str>() {
      *s
    } else {
      "No specific panic message available"
    };
    let location = if let Some(location) = panic_info.location() {
      format!(
        "Panic occurred in file '{}' at line {}",
        location.file(),
        location.line()
      )
    } else {
      "Panic location unknown".to_string()
    };
    error!("panic hook: {}\n{}", panic_message, location);
  }));
  let configuration = get_configuration(&app_env).expect("The configuration should be configured.");
  let state = init_state(&configuration)
    .await
    .expect("The AppState should be initialized");
  let application = Application::build(configuration, state).await?;
  application.run_until_stopped().await?;

  Ok(())
}
