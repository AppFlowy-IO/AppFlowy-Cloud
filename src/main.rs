use appflowy_cloud::application::{init_state, Application};
use appflowy_cloud::config::config::get_configuration;
use appflowy_cloud::telemetry::init_subscriber;
use std::panic;
use tracing::error;

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

  // let app_env: Environment = std::env::var("APP_ENVIRONMENT")
  //   .unwrap_or_else(|_| "local".to_string())
  //   .try_into()
  //   .expect("Failed to parse APP_ENVIRONMENT.");

  let conf =
    get_configuration().map_err(|e| anyhow::anyhow!("Failed to read configuration: {}", e))?;

  init_subscriber(&conf.app_env, filters);

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
  let state = init_state(&conf)
    .await
    .map_err(|e| anyhow::anyhow!("Failed to initialize application state: {}", e))?;
  let application = Application::build(conf, state).await?;
  application.run_until_stopped().await?;

  Ok(())
}
