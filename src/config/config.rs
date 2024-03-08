use anyhow::Context;
use secrecy::Secret;
use serde::Deserialize;
use sqlx::postgres::{PgConnectOptions, PgSslMode};
use std::str::FromStr;

#[derive(Clone, Debug)]
pub struct Config {
  pub app_env: Environment,
  pub db_settings: DatabaseSetting,
  pub gotrue: GoTrueSetting,
  pub application: ApplicationSetting,
  pub websocket: WebsocketSetting,
  pub redis_uri: Secret<String>,
  pub s3: S3Setting,
  pub casbin: CasbinSetting,
}

#[derive(serde::Deserialize, Clone, Debug)]
pub struct CasbinSetting {
  pub pool_size: u32,
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

#[derive(serde::Deserialize, Clone, Debug)]
pub struct GoTrueSetting {
  pub base_url: String,
  pub ext_url: String, // public url
  pub jwt_secret: Secret<String>,
  pub admin_email: String,
  pub admin_password: Secret<String>,
}

// We are using 127.0.0.1 as our host in address, we are instructing our
// application to only accept connections coming from the same machine. However,
// request from the hose machine which is not seen as local by our Docker image.
//
// Using 0.0.0.0 as host to instruct our application to accept connections from
// any network interface. So using 127.0.0.1 for our local development and set
// it to 0.0.0.0 in our Docker images.
//
#[derive(Clone, Debug)]
pub struct ApplicationSetting {
  pub port: u16,
  pub host: String,
  pub server_key: Secret<String>,
  pub use_tls: bool,
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

// Default values favor local development.
pub fn get_configuration() -> Result<Config, anyhow::Error> {
  let config = Config {
    app_env: get_env_var("APPFLOWY_ENVIRONMENT", "local")
      .parse()
      .context("fail to get APPFLOWY_ENVIRONMENT")?,
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
      database_name: get_env_var("APPFLOWY_DATABASE_NAME", "postgres"),
    },
    gotrue: GoTrueSetting {
      base_url: get_env_var("APPFLOWY_GOTRUE_BASE_URL", "http://localhost:9999"),
      ext_url: get_env_var("APPFLOWY_GOTRUE_EXT_URL", "http://localhost:9999"),
      jwt_secret: get_env_var("APPFLOWY_GOTRUE_JWT_SECRET", "hello456").into(),
      admin_email: get_env_var("APPFLOWY_GOTRUE_ADMIN_EMAIL", "admin@example.com"),
      admin_password: get_env_var("APPFLOWY_GOTRUE_ADMIN_PASSWORD", "password").into(),
    },
    application: ApplicationSetting {
      port: get_env_var("APPFLOWY_APPLICATION_PORT", "8000").parse()?,
      host: get_env_var("APPFLOWY_APPLICATION_HOST", "0.0.0.0"),
      use_tls: get_env_var("APPFLOWY_APPLICATION_USE_TLS", "false")
        .parse()
        .context("fail to get APPFLOWY_APPLICATION_USE_TLS")?,
      server_key: get_env_var("APPFLOWY_APPLICATION_SERVER_KEY", "server_key").into(),
    },
    websocket: WebsocketSetting {
      heartbeat_interval: get_env_var("APPFLOWY_WEBSOCKET_HEARTBEAT_INTERVAL", "6").parse()?,
      client_timeout: get_env_var("APPFLOWY_WEBSOCKET_CLIENT_TIMEOUT", "60").parse()?,
    },
    redis_uri: get_env_var("APPFLOWY_REDIS_URI", "redis://localhost:6379").into(),
    s3: S3Setting {
      use_minio: get_env_var("APPFLOWY_S3_USE_MINIO", "true")
        .parse()
        .context("fail to get APPFLOWY_S3_USE_MINIO")?,
      minio_url: get_env_var("APPFLOWY_S3_MINIO_URL", "http://localhost:9000"),
      access_key: get_env_var("APPFLOWY_S3_ACCESS_KEY", "minioadmin"),
      secret_key: get_env_var("APPFLOWY_S3_SECRET_KEY", "minioadmin").into(),
      bucket: get_env_var("APPFLOWY_S3_BUCKET", "appflowy"),
      region: get_env_var("APPFLOWY_S3_REGION", ""),
    },
    casbin: CasbinSetting {
      pool_size: get_env_var("APPFLOWY_CASBIN_POOL_SIZE", "8").parse()?,
    },
  };
  Ok(config)
}

fn get_env_var(key: &str, default: &str) -> String {
  std::env::var(key).unwrap_or_else(|e| {
    tracing::warn!(
      "failed to read environment variable: {}, using default value: {}",
      e,
      default
    );
    default.to_owned()
  })
}

/// The possible runtime environment for our application.
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
pub struct WebsocketSetting {
  pub heartbeat_interval: u8,
  pub client_timeout: u8,
}
