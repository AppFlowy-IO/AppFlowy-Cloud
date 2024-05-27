use clap::Parser;
use std::sync::Arc;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::Layer;

use collab_stream::client::CollabRedisStream;

use crate::consumer::OpenCollabConsumer;
use crate::indexer::PostgresIndexer;

mod collab_handle;
mod consumer;
mod error;
mod indexer;
mod watchers;

#[cfg(test)]
mod test_utils;

#[derive(Debug, Parser)]
pub struct Config {
  #[clap(long, env = "APPFLOWY_INDEXER_REDIS_URL")]
  pub redis_url: String,

  #[clap(long, env = "APPFLOWY_INDEXER_OPENAI_API_KEY")]
  pub openai_api_key: String,

  #[clap(long, env = "APPFLOWY_INDEXER_DATABASE_URL")]
  pub database_url: String,

  #[clap(
    long,
    env = "APPFLOWY_INDEXER_CONTROL_STREAM_KEY",
    default_value = "af_collab_control"
  )]
  pub control_stream_key: String,

  #[clap(long, env = "APPFLOWY_INDEXER_INGEST_INTERVAL", default_value = "30s")]
  pub ingest_interval: humantime::Duration,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
  setup_tracing();

  let config = Config::parse();
  run_server(config).await
}

async fn run_server(config: Config) -> Result<(), Box<dyn std::error::Error>> {
  let redis_client = redis::Client::open(config.redis_url)?;
  let collab_stream = CollabRedisStream::new(redis_client).await?;
  let indexer = PostgresIndexer::open(&config.openai_api_key, &config.database_url).await?;
  let consumer = OpenCollabConsumer::new(
    collab_stream,
    Arc::new(indexer),
    &config.control_stream_key,
    config.ingest_interval.into(),
  )
  .await?;
  tracing::info!("AppFlowy Indexer started!");
  consumer.await;
  Ok(())
}

fn setup_tracing() {
  if std::env::var("RUST_LOG").is_err() {
    std::env::set_var("RUST_LOG", "info");
  }

  let registry = tracing_subscriber::registry();

  registry
    .with(
      tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_filter(tracing_subscriber::EnvFilter::from_default_env()),
    )
    .init();
}
