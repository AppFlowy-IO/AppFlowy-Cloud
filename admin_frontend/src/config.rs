use tracing::warn;

#[derive(Debug, Clone)]
pub struct Config {
  pub host: String,
  pub port: u16,
  pub redis_url: String,
  pub gotrue_url: String,
  pub appflowy_cloud_url: String,
  pub oauth: OAuthConfig,
  pub path_prefix: String,
}

#[derive(Debug, Clone)]
pub struct OAuthConfig {
  pub client_id: String,
  pub client_secret: Option<String>,
  pub allowable_redirect_uris: Vec<String>,
}

impl Config {
  pub fn from_env() -> Result<Config, anyhow::Error> {
    let cfg = Config {
      host: get_or_default("ADMIN_FRONTEND_HOST", "0.0.0.0"),
      port: get_or_default("ADMIN_FRONTEND_PORT", "3000")
        .parse()
        .map_err(|e| anyhow::anyhow!("failed to parse ADMIN_FRONTEND_PORT as u16, err: {}", e))?,
      redis_url: get_or_default("ADMIN_FRONTEND_REDIS_URL", "redis://localhost:6379"),
      gotrue_url: get_or_default("ADMIN_FRONTEND_GOTRUE_URL", "http://localhost:9999"),
      appflowy_cloud_url: get_or_default(
        "ADMIN_FRONTEND_APPFLOWY_CLOUD_URL",
        "http://localhost:8000",
      ),
      oauth: OAuthConfig {
        client_id: get_or_default("ADMIN_FRONTEND_OAUTH_CLIENT_ID", "appflowy_cloud"),
        client_secret: get_optional("ADMIN_FRONTEND_OAUTH_CLIENT_SECRET"),
        allowable_redirect_uris: get_or_default(
          "ADMIN_FRONTEND_OAUTH_ALLOWABLE_REDIRECT_URIS",
          "http://localhost:3000",
        )
        .split(',')
        .map(|s| s.to_string())
        .collect(),
      },
      path_prefix: get_or_default("ADMIN_FRONTEND_PATH_PREFIX", ""),
    };
    Ok(cfg)
  }
}

fn get_or_default(key: &str, default: &str) -> String {
  std::env::var(key).unwrap_or_else(|e| {
    warn!(
      "failed to get env var: {}, err: {}, using default: {}",
      key, e, default
    );
    default.to_string()
  })
}

fn get_optional(key: &str) -> Option<String> {
  let s = match std::env::var(key) {
    Ok(s) => s,
    Err(err) => {
      warn!("failed to get env var: {}, err: {}", key, err);
      return None;
    },
  };
  if s.is_empty() {
    warn!("env var: {} is empty", key);
    None
  } else {
    Some(s)
  }
}
