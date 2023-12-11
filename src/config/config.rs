use config::{Config as InnerConfig, FileFormat};
use secrecy::Secret;
use serde::Deserialize;
use serde_aux::field_attributes::deserialize_number_from_string;
use sqlx::postgres::{PgConnectOptions, PgSslMode};
use std::convert::TryFrom;
use std::path::PathBuf;

#[derive(serde::Deserialize, Clone, Debug)]
pub struct Config {
  pub database: DatabaseSetting,
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
  pub secret_key: String,
  pub bucket: String,
  pub region: String,
}

#[derive(serde::Deserialize, Clone, Debug)]
pub struct GoTrueSetting {
  pub base_url: String,
  pub ext_url: String, // public url
  pub jwt_secret: Secret<String>,
  pub admin_email: String,
  pub admin_password: String,
}

// We are using 127.0.0.1 as our host in address, we are instructing our
// application to only accept connections coming from the same machine. However,
// request from the hose machine which is not seen as local by our Docker image.
//
// Using 0.0.0.0 as host to instruct our application to accept connections from
// any network interface. So using 127.0.0.1 for our local development and set
// it to 0.0.0.0 in our Docker images.
//
#[derive(serde::Deserialize, Clone, Debug)]
pub struct ApplicationSetting {
  #[serde(deserialize_with = "deserialize_number_from_string")]
  pub port: u16,
  pub host: String,
  pub data_dir: PathBuf,
  pub server_key: Secret<String>,
  pub tls_config: Option<TlsConfig>,
}

impl ApplicationSetting {
  pub fn use_https(&self) -> bool {
    match &self.tls_config {
      None => false,
      Some(config) => match config {
        TlsConfig::NoTls => false,
        TlsConfig::SelfSigned => true,
      },
    }
  }

  pub fn rocksdb_db_dir(&self) -> PathBuf {
    self.data_dir.join("rocksdb")
  }
}

#[derive(serde::Deserialize, Clone, Debug)]
#[serde(rename_all = "snake_case")]
pub enum TlsConfig {
  NoTls,
  SelfSigned,
}

#[derive(serde::Deserialize, Clone, Debug)]
pub struct DatabaseSetting {
  pub username: String,
  pub password: String,
  #[serde(deserialize_with = "deserialize_number_from_string")]
  pub port: u16,
  pub host: String,
  pub database_name: String,
  pub require_ssl: bool,
  pub max_connections: u32,
}

impl DatabaseSetting {
  pub fn without_db(&self) -> PgConnectOptions {
    let ssl_mode = if self.require_ssl {
      PgSslMode::Require
    } else {
      PgSslMode::Prefer
    };
    PgConnectOptions::new()
      .host(&self.host)
      .username(&self.username)
      .password(&self.password)
      .port(self.port)
      .ssl_mode(ssl_mode)
  }

  pub fn with_db(&self) -> PgConnectOptions {
    self.without_db().database(&self.database_name)
  }

  /// Generate a postgresql connection string from the database settings.
  pub fn to_pg_url(&self) -> String {
    let ssl_mode = if self.require_ssl {
      "require"
    } else {
      "prefer"
    };
    format!(
      "postgres://{}:{}@{}:{}/{}?sslmode={}",
      self.username, self.password, self.host, self.port, self.database_name, ssl_mode
    )
  }
}

pub fn get_configuration(app_env: &Environment) -> Result<Config, config::ConfigError> {
  let base_path = std::env::current_dir().expect("Failed to determine the current directory");
  let configuration_dir = base_path.join("configuration");

  let builder = InnerConfig::builder()
        .set_default("default", "1")?
        .add_source(
            config::File::from(configuration_dir.join("base"))
                .required(true)
                .format(FileFormat::Yaml),
        )
        .add_source(
            config::File::from(configuration_dir.join(app_env.as_str()))
                .required(true)
                .format(FileFormat::Yaml),
        )
        // Add in settings from environment variables (with a prefix of APP and '__' as
        // separator) E.g. `APP__APPLICATION__PORT=5001 would set
        // `Settings.application.port`
        .add_source(config::Environment::with_prefix("app").separator("__"));

  let config = builder.build()?;
  config.try_deserialize()
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

impl TryFrom<String> for Environment {
  type Error = String;

  fn try_from(s: String) -> Result<Self, Self::Error> {
    match s.to_lowercase().as_str() {
      "local" => Ok(Self::Local),
      "production" => Ok(Self::Production),
      other => Err(format!(
        "{} is not a supported environment. Use either `local` or `production`.",
        other
      )),
    }
  }
}
#[derive(serde::Deserialize, Clone, Debug)]
pub struct WebsocketSetting {
  pub heartbeat_interval: u8,
  pub client_timeout: u8,
}
