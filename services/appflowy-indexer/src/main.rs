use appflowy_ai_client::client::AppFlowyAIClient;
use clap::Parser;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::Layer;

use collab_stream::client::CollabRedisStream;

use crate::consumer::OpenCollabConsumer;

pub mod collab_handle;
pub mod consumer;
mod document_watcher;
pub mod error;

#[cfg(test)]
mod test_utils;

#[derive(Debug, Parser)]
pub struct Config {
  #[clap(long, env = "APPFLOWY_INDEXER_REDIS_URL")]
  pub redis_url: String,

  #[clap(long, env = "APPFLOWY_INDEXER_AI_URL")]
  pub appflowy_ai_url: String,

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
  let ai_client = AppFlowyAIClient::new(&config.appflowy_ai_url);
  let consumer = OpenCollabConsumer::new(
    collab_stream,
    ai_client,
    &config.control_stream_key,
    config.ingest_interval.into(),
  )
  .await?;
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
