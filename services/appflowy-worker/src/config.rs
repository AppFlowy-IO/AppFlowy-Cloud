use anyhow::{Context, Error};
use infra::env_util::get_env_var;
use mailer::config::MailerSetting;
use secrecy::Secret;
use serde::Deserialize;
use sqlx::postgres::{PgConnectOptions, PgSslMode};
use std::{fmt::Display, str::FromStr};

#[derive(Debug, Clone)]
pub struct Config {
  pub app_env: Environment,
  pub redis_url: String,
  pub db_settings: DatabaseSetting,
  pub s3_setting: S3Setting,
  pub mailer: MailerSetting,
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
      mailer: MailerSetting {
        smtp_host: get_env_var("APPFLOWY_MAILER_SMTP_HOST", "smtp.gmail.com"),
        smtp_port: get_env_var("APPFLOWY_MAILER_SMTP_PORT", "465").parse()?,
        smtp_email: get_env_var("APPFLOWY_MAILER_SMTP_EMAIL", "sender@example.com"),
        // `smtp_username` could be the same as `smtp_email`, but may not have to be.
        // For example:
        //  - Azure Communication services uses a string of the format <resource name>.<app id>.<tenant id>
        //  - SendGrid uses the string apikey
        // Adapted from: https://github.com/AppFlowy-IO/AppFlowy-Cloud/issues/984
        smtp_username: get_env_var("APPFLOWY_MAILER_SMTP_USERNAME", "sender@example.com"),
        smtp_password: get_env_var("APPFLOWY_MAILER_SMTP_PASSWORD", "password").into(),
        smtp_tls_kind: get_env_var("APPFLOWY_MAILER_SMTP_TLS_KIND", "wrapper"),
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

impl Display for DatabaseSetting {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    let masked_pg_conn_opts = self.pg_conn_opts.clone().password("********");
    write!(
      f,
      "DatabaseSetting {{ pg_conn_opts: {:?}, require_ssl: {}, max_connections: {} }}",
      masked_pg_conn_opts, self.require_ssl, self.max_connections
    )
  }
}

#[allow(dead_code)]
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
