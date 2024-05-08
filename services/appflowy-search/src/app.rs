use crate::error::Error;
use clap::Parser;
use redis::aio::ConnectionManager;
use sqlx::postgres::PgConnectOptions;
use sqlx::PgPool;
use std::str::FromStr;
use std::sync::Arc;

pub type OpenAiClient = Arc<openai_api_rs::v1::api::Client>;

#[derive(Clone)]
pub struct AppState {
  pub redis_client: ConnectionManager,
  pub pg_pool: PgPool,
  pub embed_model_client: OpenAiClient,
}

impl AppState {
  pub async fn init(config: Config) -> Result<Self, Error> {
    let pg_pool = PgPool::connect_with(config.db_options()?).await?;
    let redis_client = redis::Client::open(config.redis_url)?
      .get_connection_manager()
      .await?;
    let embed_model_client = Arc::new(openai_api_rs::v1::api::Client::new(config.openai_api_key));
    Ok(AppState {
      redis_client,
      pg_pool,
      embed_model_client,
    })
  }

  pub async fn run(self) -> Result<(), Error> {
    todo!()
  }
}

#[derive(Parser, Debug, Clone)]
pub struct Config {
  #[arg(
    long,
    env = "APPFLOWY_SEARCH_REDIS_URL",
    default_value = "redis://localhost:6379"
  )]
  pub redis_url: String,

  #[arg(
    long,
    env = "APPFLOWY_SEARCH_DATABASE_URL",
    default_value = "postgres://postgres:password@localhost:5432/postgres"
  )]
  pub database_url: String,

  #[arg(
    long,
    env = "APPFLOWY_SEARCH_DATABASE_REQUIRE_SSL",
    default_value = "false"
  )]
  pub database_require_ssl: bool,

  #[arg(
    long,
    env = "APPFLOWY_SEARCH_DATABASE_NAME",
    default_value = "postgres"
  )]
  pub database_name: String,

  #[arg(long, env = "APPFLOWY_SEARCH_OPENAI_API_KEY")]
  pub openai_api_key: String,
}

impl Config {
  pub fn db_options(&self) -> Result<PgConnectOptions, sqlx::Error> {
    PgConnectOptions::from_str(&self.database_url)
  }
}
