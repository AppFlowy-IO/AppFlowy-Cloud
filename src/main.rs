use appflowy_cloud::application::{init_state, Application};
use appflowy_cloud::config::config::get_configuration;
use appflowy_cloud::telemetry::{get_subscriber, init_subscriber};

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

  let subscriber = get_subscriber(
    "appflowy_cloud".to_string(),
    Some(filters.join(",")),
    std::io::stdout,
  );
  init_subscriber(subscriber);

  let configuration = get_configuration().expect("The configuration should be configured.");
  let state = init_state(&configuration)
    .await
    .expect("The AppState should be initialized");
  let application = Application::build(configuration, state).await?;
  application.run_until_stopped().await?;

  Ok(())
}
