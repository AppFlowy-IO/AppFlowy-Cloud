use anyhow::{Context, Error};
use infra::env_util::get_env_var;
use secrecy::Secret;
use serde::Deserialize;
use sqlx::postgres::{PgConnectOptions, PgSslMode};
use std::str::FromStr;

#[derive(Debug, Clone)]
pub struct Config {
  pub app_env: Environment,
  pub redis_url: String,
  pub db_settings: DatabaseSetting,
  pub s3_setting: S3Setting,
}

impl Config {
  pub fn from_env() -> Result<Self, Error> {
    Ok(Config {
      app_env: get_env_var("APPFLOWY_WORKER_ENVIRONMENT", "local")
        .parse()
        .context("fail to get APPFLOWY_WORKER_ENVIRONMENT")?,
      redis_url: get_env_var("APPFLOWY_WORKER_REDIS_URL", "redis://localhost:6379"),
      db_settings: DatabaseSetting {
        pg_conn_opts: PgConnectOptions::from_str(&get_env_var(
          "APPFLOWY_WORKER_DATABASE_URL",
          "postgres://postgres:password@localhost:5432/postgres",
        ))?,
        require_ssl: get_env_var("APPFLOWY_WORKER_DATABASE_REQUIRE_SSL", "false")
          .parse()
          .context("fail to get APPFLOWY_WORKER_DATABASE_REQUIRE_SSL")?,
        max_connections: get_env_var("APPFLOWY_WORKER_DATABASE_MAX_CONNECTIONS", "10")
          .parse()
          .context("fail to get APPFLOWY_WORKER_DATABASE_MAX_CONNECTIONS")?,
        database_name: get_env_var("APPFLOWY_WORKER_DATABASE_NAME", "postgres"),
      },
      s3_setting: S3Setting {
        use_minio: get_env_var("APPFLOWY_S3_USE_MINIO", "true")
          .parse()
          .context("fail to get APPFLOWY_S3_USE_MINIO")?,
        minio_url: get_env_var("APPFLOWY_S3_MINIO_URL", "http://localhost:9000"),
        access_key: get_env_var("APPFLOWY_S3_ACCESS_KEY", "minioadmin"),
        secret_key: get_env_var("APPFLOWY_S3_SECRET_KEY", "minioadmin").into(),
        bucket: get_env_var("APPFLOWY_S3_BUCKET", "appflowy"),
        region: get_env_var("APPFLOWY_S3_REGION", ""),
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

#[derive(serde::Deserialize, Clone, Debug)]
pub struct S3Setting {
  pub use_minio: bool,
  pub minio_url: String,
  pub access_key: String,
  pub secret_key: Secret<String>,
  pub bucket: String,
  pub region: String,
}
