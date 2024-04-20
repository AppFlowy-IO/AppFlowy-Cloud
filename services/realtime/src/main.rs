use realtime::application::{init_state, Application};
use realtime::config::get_configuration;

#[actix_web::main]
async fn main() -> anyhow::Result<()> {
  dotenvy::dotenv().ok();
  let conf =
    get_configuration().map_err(|e| anyhow::anyhow!("Failed to read configuration: {}", e))?;
  let state = init_state(&conf).await?;
  let application = Application::build(conf, state).await?;
  application.run_until_stopped().await?;
  Ok(())
}
