use tracing::warn;

#[derive(Debug, Clone)]
pub struct Config {
  pub redis_url: String,
  pub gotrue_url: String,
}

impl Config {
  pub fn from_env() -> Self {
    Config {
      redis_url: get_or_default("ADMIN_FRONTEND_REDIS_URL", "redis://localhost:6379"),
      gotrue_url: get_or_default("ADMIN_FRONTEND_GOTRUE_URL", "http://localhost:9999"),
    }
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
