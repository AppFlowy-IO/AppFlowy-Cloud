use clap::Parser;
use std::sync::{Arc, Once};
use tracing::subscriber::set_global_default;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::EnvFilter;

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

  #[clap(
    long,
    env = "APPFLOWY_INDEXER_ENVIRONMENT",
    default_value = "local",
    value_enum
  )]
  pub app_env: Environment,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
  dotenvy::dotenv().ok();

  let config = Config::parse();
  init_subscriber(&config.app_env);
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

fn init_subscriber(app_env: &Environment) {
  static START: Once = Once::new();
  START.call_once(|| {
    let level = std::env::var("RUST_LOG").unwrap_or("info".to_string());
    let mut filters = vec![];
    filters.push(format!("appflowy_history={}", level));
    let env_filter = EnvFilter::new(filters.join(","));

    let builder = tracing_subscriber::fmt()
      .with_target(true)
      .with_max_level(tracing::Level::TRACE)
      .with_thread_ids(false)
      .with_file(false);

    match app_env {
      Environment::Local => {
        let subscriber = builder
          .with_ansi(true)
          .with_target(false)
          .with_file(false)
          .pretty()
          .finish()
          .with(env_filter);
        set_global_default(subscriber).unwrap();
      },
      Environment::Production => {
        let subscriber = builder.json().finish().with(env_filter);
        set_global_default(subscriber).unwrap();
      },
    }
  });
}

#[derive(Clone, Debug, clap::ValueEnum)]
pub enum Environment {
  Local,
  Production,
}
