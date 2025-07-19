use std::env::VarError;

pub fn get_env_var(key: &str, default: &str) -> String {
  std::env::var(key).unwrap_or_else(|err| {
    match err {
      VarError::NotPresent => {
        tracing::info!("using default environment variable {}:{}", key, default)
      },
      VarError::NotUnicode(_) => {
        tracing::error!(
          "{} is not a valid UTF-8 string, use default value:{}",
          key,
          default
        );
      },
    }
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
    Err(_) => {
      tracing::info!("using default environment variable {}:None", key);
      None
    },
  }
}

pub fn get_required_env_var(key: &str) -> String {
  std::env::var(key).unwrap_or_else(|err| {
    match err {
      VarError::NotPresent => {
        panic!("missing required environment variable: {}", key);
      },
      VarError::NotUnicode(_) => {
        panic!("{} is not a valid UTF-8 string", key);
      },
    }
  })
}
