use appflowy_collaborate::application::{init_state, Application};
use appflowy_collaborate::config::get_configuration;

#[actix_web::main]
async fn main() -> anyhow::Result<()> {
  // Load environment variables from .env file
  dotenvy::dotenv().ok();

  let conf =
    get_configuration().map_err(|e| anyhow::anyhow!("Failed to read configuration: {}", e))?;

  let (tx, rx) = tokio::sync::mpsc::channel(1000);
  let state = init_state(&conf, tx)
    .await
    .map_err(|e| anyhow::anyhow!("Failed to initialize application state: {}", e))?;
  let application = Application::build(conf, state, rx).await?;
  application.run_until_stopped().await?;
  Ok(())
}
