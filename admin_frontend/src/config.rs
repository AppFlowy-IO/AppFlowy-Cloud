use tracing::warn;

#[derive(Debug, Clone)]
pub struct Config {
  pub host: String,
  pub port: u16,
  pub redis_url: String,
  pub gotrue_url: String,
  pub appflowy_cloud_url: String,
  pub oauth: OAuthConfig,
}

#[derive(Debug, Clone)]
pub struct OAuthConfig {
  pub client_id: String,
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
        allowable_redirect_uris: get_or_default(
          "ADMIN_FRONTEND_OAUTH_ALLOWABLE_REDIRECT_URIS",
          "http://localhost:3000",
        )
        .split(',')
        .map(|s| s.to_string())
        .collect(),
      },
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
