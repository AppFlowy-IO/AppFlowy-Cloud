use anyhow::{Context, Error};
use infra::env_util::get_env_var;
use sqlx::postgres::{PgConnectOptions, PgSslMode};
use std::str::FromStr;

#[derive(Debug, Clone)]
pub struct Config {
  pub redis_url: String,
  pub db_settings: DatabaseSetting,
}

impl Config {
  pub fn from_env() -> Result<Self, Error> {
    Ok(Config {
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
