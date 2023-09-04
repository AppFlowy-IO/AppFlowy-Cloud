use appflowy_cloud::application::{init_state, Application};
use appflowy_cloud::config::config::get_configuration;
use appflowy_cloud::telemetry::{get_subscriber, init_subscriber};

#[actix_web::main]
async fn main() -> anyhow::Result<()> {
  let subscriber = get_subscriber(
    "appflowy_cloud".to_string(),
    "info".to_string(),
    std::io::stdout,
  );
  init_subscriber(subscriber);

  let configuration = get_configuration().expect("Failed to read configuration.");
  let state = init_state(&configuration).await;
  let application = Application::build(configuration, state).await?;
  application.run_until_stopped().await?;

  Ok(())
}
