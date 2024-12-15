pub fn get_env_var(key: &str, default: &str) -> String {
  std::env::var(key).unwrap_or_else(|e| {
    tracing::debug!(
      "failed to read environment variable:{}:{}, using default value: {}",
      e,
      key,
      default
    );
    default.to_owned()
  })
}

/// Optionally get an environment variable.
/// if value is empty, return None.
pub fn get_env_var_opt(key: &str) -> Option<String> {
  match std::env::var(key) {
    Ok(val) => {
      if val.is_empty() {
        None
      } else {
        Some(val)
      }
    },
    Err(e) => {
      tracing::warn!("failed to read environment variable: {}, None set", e);
      None
    },
  }
}
