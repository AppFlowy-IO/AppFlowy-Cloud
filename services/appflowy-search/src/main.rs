mod app;
mod error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
  use clap::Parser;

  let config = app::Config::parse();
  let app = app::AppState::init(config).await?;
  app.run().await?;

  Ok(())
}
