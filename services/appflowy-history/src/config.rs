use anyhow::{Context, Error};
use collab_stream::client::CONTROL_STREAM_KEY;
use infra::env_util::get_env_var;
use serde::Deserialize;
use sqlx::postgres::{PgConnectOptions, PgSslMode};
use std::str::FromStr;

#[derive(Debug, Clone)]
pub struct Config {
  pub app_env: Environment,
  pub redis_url: String,
  pub db_settings: DatabaseSetting,
  pub stream_settings: StreamSetting,
}

impl Config {
  pub fn from_env() -> Result<Self, Error> {
    Ok(Config {
      app_env: get_env_var("APPFLOWY_HISTORY_ENVIRONMENT", "local")
        .parse()
        .context("fail to get APPFLOWY_HISTORY_ENVIRONMENT")?,
      redis_url: get_env_var("APPFLOWY_HISTORY_REDIS_URL", "redis://localhost:6379"),
      db_settings: DatabaseSetting {
        pg_conn_opts: PgConnectOptions::from_str(&get_env_var(
          "APPFLOWY_HISTORY_DATABASE_URL",
          "postgres://postgres:password@localhost:5432/postgres",
        ))?,
        require_ssl: get_env_var("APPFLOWY_HISTORY_DATABASE_REQUIRE_SSL", "false")
          .parse()
          .context("fail to get APPFLOWY_HISTORY_DATABASE_REQUIRE_SSL")?,
        max_connections: get_env_var("APPFLOWY_HISTORY_DATABASE_MAX_CONNECTIONS", "20")
          .parse()
          .context("fail to get APPFLOWY_HISTORY_DATABASE_MAX_CONNECTIONS")?,
        database_name: get_env_var("APPFLOWY_HISTORY_DATABASE_NAME", "postgres"),
      },
      stream_settings: StreamSetting {
        control_key: CONTROL_STREAM_KEY.to_string(),
      },
    })
  }
}

#[derive(Clone, Debug)]
pub struct DatabaseSetting {
  pub pg_conn_opts: PgConnectOptions,
  pub require_ssl: bool,
  pub max_connections: u32,
  pub database_name: String,
}

impl DatabaseSetting {
  pub fn without_db(&self) -> PgConnectOptions {
    let ssl_mode = if self.require_ssl {
      PgSslMode::Require
    } else {
      PgSslMode::Prefer
    };
    let options = self.pg_conn_opts.clone();
    options.ssl_mode(ssl_mode)
  }

  pub fn with_db(&self) -> PgConnectOptions {
    self.without_db().database(&self.database_name)
  }
}

#[derive(Debug, Clone)]
pub struct StreamSetting {
  /// The key of the stream that contains control event, [CollabControlEvent].
  pub control_key: String,
}

#[derive(Clone, Debug, Deserialize)]
pub enum Environment {
  Local,
  Production,
}

impl Environment {
  pub fn as_str(&self) -> &'static str {
    match self {
      Environment::Local => "local",
      Environment::Production => "production",
    }
  }
}

impl FromStr for Environment {
  type Err = anyhow::Error;

  fn from_str(s: &str) -> Result<Self, Self::Err> {
    match s.to_lowercase().as_str() {
      "local" => Ok(Self::Local),
      "production" => Ok(Self::Production),
      other => anyhow::bail!(
        "{} is not a supported environment. Use either `local` or `production`.",
        other
      ),
    }
  }
}
