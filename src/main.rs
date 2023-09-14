use appflowy_cloud::application::{init_state, init_storage, Application};
use appflowy_cloud::config::config::get_configuration;
use appflowy_cloud::telemetry::{get_subscriber, init_subscriber};

#[actix_web::main]
async fn main() -> anyhow::Result<()> {
  let level = std::env::var("RUST_LOG").unwrap_or("info".to_string());
  let mut filters = vec![];
  filters.push(format!("actix_web={}", level));
  filters.push(format!("collab={}", level));
  filters.push(format!("collab_sync={}", level));
  filters.push(format!("appflowy_cloud={}", level));
  filters.push(format!("collab_plugins={}", level));
  filters.push(format!("realtime={}", level));

  let subscriber = get_subscriber(
    "appflowy_cloud".to_string(),
    Some(filters.join(",")),
    std::io::stdout,
  );
  init_subscriber(subscriber);

  let configuration = get_configuration().expect("Failed to read configuration.");
  let state = init_state(&configuration).await;
  let storage = init_storage(&configuration, state.pg_pool.clone()).await;
  let application = Application::build(configuration, state, storage).await?;
  application.run_until_stopped().await?;

  Ok(())
}
