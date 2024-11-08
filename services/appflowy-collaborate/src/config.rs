use anyhow::Context;
use secrecy::Secret;
use semver::Version;
use serde::Deserialize;
use sqlx::postgres::{PgConnectOptions, PgSslMode};
use std::fmt::Display;
use std::str::FromStr;

#[derive(Clone, Debug)]
pub struct Config {
  pub app_env: Environment,
  pub application: ApplicationSetting,
  pub websocket: WebsocketSetting,
  pub db_settings: DatabaseSetting,
  pub gotrue: GoTrueSetting,
  pub collab: CollabSetting,
  pub redis_uri: Secret<String>,
  pub redis_worker_count: usize,
  pub ai: AISettings,
}

#[derive(Clone, Debug)]
pub struct ApplicationSetting {
  pub port: u16,
  pub host: String,
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

#[derive(Clone, Debug)]
pub struct AISettings {
  pub port: u16,
  pub host: String,
}

impl AISettings {
  pub fn url(&self) -> String {
    format!("http://{}:{}", self.host, self.port)
  }
}

#[derive(Clone, Debug)]
pub struct WebsocketSetting {
  pub heartbeat_interval: u8,
  pub client_timeout: u8,
  pub min_client_version: Version,
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

#[derive(Clone, Debug, Deserialize)]
pub struct GoTrueSetting {
  pub jwt_secret: Secret<String>,
}

#[derive(Clone, Debug)]
pub struct CollabSetting {
  pub group_persistence_interval_secs: u64,
  pub group_prune_grace_period_secs: u64,
  pub edit_state_max_count: u32,
  pub edit_state_max_secs: i64,
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
    app_env: get_env_var("APPFLOWY_ENVIRONMENT", "local")
      .parse()
      .context("fail to get APPFLOWY_ENVIRONMENT")?,
    application: ApplicationSetting {
      port: get_env_var("APPFLOWY_COLLAB_SERVICE_PORT", "8001").parse()?,
      host: get_env_var("APPFLOWY_COLLAB_SERVICE_HOST", "0.0.0.0"),
    },
    websocket: WebsocketSetting {
      heartbeat_interval: get_env_var("APPFLOWY_WEBSOCKET_HEARTBEAT_INTERVAL", "6").parse()?,
      client_timeout: get_env_var("APPFLOWY_WEBSOCKET_CLIENT_TIMEOUT", "60").parse()?,
      min_client_version: get_env_var("APPFLOWY_WEBSOCKET_CLIENT_MIN_VERSION", "0.5.0").parse()?,
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
    collab: CollabSetting {
      group_persistence_interval_secs: get_env_var(
        "APPFLOWY_COLLAB_GROUP_PERSISTENCE_INTERVAL",
        "60",
      )
      .parse()?,
      group_prune_grace_period_secs: get_env_var("APPFLOWY_COLLAB_GROUP_GRACE_PERIOD_SECS", "60")
        .parse()?,
      edit_state_max_count: get_env_var("APPFLOWY_COLLAB_EDIT_STATE_MAX_COUNT", "100").parse()?,
      edit_state_max_secs: get_env_var("APPFLOWY_COLLAB_EDIT_STATE_MAX_SECS", "60").parse()?,
    },
    redis_uri: get_env_var("APPFLOWY_REDIS_URI", "redis://localhost:6379").into(),
    redis_worker_count: get_env_var("APPFLOWY_REDIS_WORKERS", "60").parse()?,
    ai: AISettings {
      port: get_env_var("APPFLOWY_AI_SERVER_PORT", "5001").parse()?,
      host: get_env_var("APPFLOWY_AI_SERVER_HOST", "localhost"),
    },
  };
  Ok(config)
}
