#[derive(Clone, Debug)]
pub struct Config {
  pub application: ApplicationSetting,
  pub ws_client: WSClientConfig,
}

#[derive(Clone, Debug)]
pub struct ApplicationSetting {
  pub port: u16,
  pub host: String,
}

#[derive(Clone, Debug)]
pub struct WSClientConfig {
  pub url: String,
  pub buffer_capacity: usize,
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
      port: get_env_var("APPFLOWY_APPLICATION_PORT", "8001").parse()?,
      host: get_env_var("APPFLOWY_APPLICATION_HOST", "0.0.0.0"),
    },
    ws_client: WSClientConfig {
      url: get_env_var("APPFLOWY_WS_SERVER_URL", "ws://localhost:8000/ws/v1"),
      buffer_capacity: get_env_var("APPFLOWY_WS_CLIENT_BUFFER_CAPACITY", "1000").parse()?,
    },
  };
  Ok(config)
}
