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
