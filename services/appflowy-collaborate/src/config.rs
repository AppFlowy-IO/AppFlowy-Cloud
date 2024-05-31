use std::fmt::Display;
use std::str::FromStr;

use anyhow::Context;
use secrecy::Secret;
use sqlx::postgres::{PgConnectOptions, PgSslMode};

#[derive(Clone, Debug)]
pub struct Config {
  pub application: ApplicationSetting,
  pub websocket: WebsocketSetting,
  pub db_settings: DatabaseSetting,
  pub gotrue: GoTrueSetting,
  pub redis_uri: Secret<String>,
}

#[derive(Clone, Debug)]
pub struct ApplicationSetting {
  pub port: u16,
  pub host: String,
}

#[derive(Clone, Debug)]
pub struct WebsocketSetting {
  pub heartbeat_interval: u8,
  pub client_timeout: u8,
}

#[derive(Clone, Debug)]
pub struct DatabaseSetting {
  pub pg_conn_opts: PgConnectOptions,
  pub require_ssl: bool,
  /// PostgreSQL has a maximum of 115 connections to the database, 15 connections are reserved to
  /// the super user to maintain the integrity of the PostgreSQL database, and 100 PostgreSQL
  /// connections are reserved for system applications.
  /// When we exceed the limit of the database connection, then it shows an error message.
  pub max_connections: u32,
}

impl Display for DatabaseSetting {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(
      f,
      "DatabaseSetting {{ pg_conn_opts: {:?}, require_ssl: {}, max_connections: {} }}",
      self.pg_conn_opts, self.require_ssl, self.max_connections
    )
  }
}

impl DatabaseSetting {
  pub fn pg_connect_options(&self) -> PgConnectOptions {
    let ssl_mode = if self.require_ssl {
      PgSslMode::Require
    } else {
      PgSslMode::Prefer
    };
    let options = self.pg_conn_opts.clone();
    options.ssl_mode(ssl_mode)
  }
}

#[derive(serde::Deserialize, Clone, Debug)]
pub struct GoTrueSetting {
  pub jwt_secret: Secret<String>,
}

pub fn get_env_var(key: &str, default: &str) -> String {
  std::env::var(key).unwrap_or_else(|e| {
    tracing::warn!(
      "failed to read environment variable: {}, using default value: {}",
      e,
      default
    );
    default.to_owned()
  })
}

pub fn get_configuration() -> Result<Config, anyhow::Error> {
  let config = Config {
    application: ApplicationSetting {
      port: get_env_var("APPFLOWY_COLLAB_SERVICE_PORT", "8001").parse()?,
      host: get_env_var("APPFLOWY_COLLAB_SERVICE_HOST", "0.0.0.0"),
    },
    websocket: WebsocketSetting {
      heartbeat_interval: get_env_var("APPFLOWY_WEBSOCKET_HEARTBEAT_INTERVAL", "6").parse()?,
      client_timeout: get_env_var("APPFLOWY_WEBSOCKET_CLIENT_TIMEOUT", "60").parse()?,
    },
    db_settings: DatabaseSetting {
      pg_conn_opts: PgConnectOptions::from_str(&get_env_var(
        "APPFLOWY_DATABASE_URL",
        "postgres://postgres:password@localhost:5432/postgres",
      ))?,
      require_ssl: get_env_var("APPFLOWY_DATABASE_REQUIRE_SSL", "false")
        .parse()
        .context("fail to get APPFLOWY_DATABASE_REQUIRE_SSL")?,
      max_connections: get_env_var("APPFLOWY_DATABASE_MAX_CONNECTIONS", "40")
        .parse()
        .context("fail to get APPFLOWY_DATABASE_MAX_CONNECTIONS")?,
    },
    gotrue: GoTrueSetting {
      jwt_secret: get_env_var("APPFLOWY_GOTRUE_JWT_SECRET", "hello456").into(),
    },
    redis_uri: get_env_var("APPFLOWY_REDIS_URI", "redis://localhost:6379").into(),
  };
  Ok(config)
}
