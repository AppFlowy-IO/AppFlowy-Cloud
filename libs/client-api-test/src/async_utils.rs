use crate::test_client_config::RetryConfig;
use anyhow::{anyhow, Error};
use shared_entity::response::{AppResponseError, ErrorCode};
use std::future::Future;
use tokio::time::{sleep, timeout};
use tracing::{info, warn};

/// Executes a future with retry logic and exponential backoff
pub async fn retry_with_backoff<T, F, Fut>(operation: F, config: RetryConfig) -> Result<T, Error>
where
  F: Fn() -> Fut,
  Fut: Future<Output = Result<T, Error>>,
{
  let mut attempt = 0;
  let mut delay = config.poll_interval;
  let max_delay = config.poll_interval * 10; // Cap maximum delay

  let result = timeout(config.timeout, async {
    loop {
      match operation().await {
        Ok(result) => return Ok(result),
        Err(e) if attempt >= config.max_retries => {
          return Err(anyhow!(
            "Max retries ({}) exceeded. Last error: {}",
            config.max_retries,
            e
          ));
        },
        Err(e) => {
          attempt += 1;
          warn!(
            "Operation failed (attempt {}/{}): {}. Retrying in {:?}",
            attempt, config.max_retries, e, delay
          );

          sleep(delay).await;
          // Exponential backoff with jitter
          delay = std::cmp::min(delay * 2, max_delay);
        },
      }
    }
  })
  .await;

  match result {
    Ok(value) => value,
    Err(_) => Err(anyhow!("Operation timed out after {:?}", config.timeout)),
  }
}

/// Executes a future with retry logic and constant intervals (no exponential backoff)
pub async fn retry_with_constant_interval<T, F, Fut>(
  operation: F,
  config: RetryConfig,
) -> Result<T, Error>
where
  F: Fn() -> Fut,
  Fut: Future<Output = Result<T, Error>>,
{
  let mut attempt = 0;

  let result = timeout(config.timeout, async {
    loop {
      match operation().await {
        Ok(result) => return Ok(result),
        Err(e) if attempt >= config.max_retries => {
          return Err(anyhow!(
            "Max retries ({}) exceeded. Last error: {}",
            config.max_retries,
            e
          ));
        },
        Err(e) => {
          attempt += 1;
          warn!(
            "Operation failed (attempt {}/{}): {}. Retrying in {:?}",
            attempt, config.max_retries, e, config.poll_interval
          );

          sleep(config.poll_interval).await;
        },
      }
    }
  })
  .await;

  match result {
    Ok(value) => value,
    Err(_) => Err(anyhow!("Operation timed out after {:?}", config.timeout)),
  }
}

/// Executes an API operation with retry logic and constant intervals, returning AppResponseError
pub async fn retry_api_with_constant_interval<T, F, Fut>(
  operation: F,
  config: RetryConfig,
) -> Result<T, shared_entity::response::AppResponseError>
where
  F: Fn() -> Fut,
  Fut: Future<Output = Result<T, shared_entity::response::AppResponseError>>,
{
  let mut attempt = 0;

  let result = timeout(config.timeout, async {
    loop {
      match operation().await {
        Ok(result) => {
          info!(
            "Operation succeeded (attempt {}/{})",
            attempt + 1,
            config.max_retries,
          );
          return Ok(result);
        },
        Err(e) if attempt >= config.max_retries => {
          return Err(AppResponseError {
            code: ErrorCode::Internal,
            message: format!(
              "Max retries ({}) exceeded. Last error: {}",
              config.max_retries, e.message
            )
            .into(),
          });
        },
        Err(e) => {
          attempt += 1;
          warn!(
            "Operation failed (attempt {}/{}): {}. Retrying in {:?}",
            attempt, config.max_retries, e.message, config.poll_interval
          );

          sleep(config.poll_interval).await;
        },
      }
    }
  })
  .await;

  match result {
    Ok(value) => value,
    Err(_) => Err(shared_entity::response::AppResponseError {
      code: shared_entity::response::ErrorCode::RequestTimeout,
      message: format!("Operation timed out after {:?}", config.timeout).into(),
    }),
  }
}

/// Polls a condition until it returns true or timeout is reached
pub async fn poll_until<F, Fut>(condition: F, config: RetryConfig) -> Result<(), Error>
where
  F: Fn() -> Fut,
  Fut: Future<Output = bool>,
{
  retry_with_backoff(
    || async {
      if condition().await {
        Ok(())
      } else {
        Err(anyhow!("Condition not met"))
      }
    },
    config,
  )
  .await
}

/// Polls a function until it returns Some(T) or timeout is reached
pub async fn poll_until_some<T, F, Fut>(operation: F, config: RetryConfig) -> Result<T, Error>
where
  F: Fn() -> Fut,
  Fut: Future<Output = Option<T>>,
{
  retry_with_backoff(
    || async {
      match operation().await {
        Some(value) => Ok(value),
        None => Err(anyhow!("Operation returned None")),
      }
    },
    config,
  )
  .await
}

/// Polls a function until it returns Ok(T) or timeout is reached
pub async fn poll_until_ok<T, E, F, Fut>(operation: F, config: RetryConfig) -> Result<T, Error>
where
  F: Fn() -> Fut,
  Fut: Future<Output = Result<T, E>>,
  E: std::error::Error + Send + Sync + 'static,
{
  retry_with_backoff(
    || async {
      operation()
        .await
        .map_err(|e| anyhow!("Operation failed: {}", e))
    },
    config,
  )
  .await
}

/// Waits for multiple conditions to be met within the timeout
pub async fn wait_for_all_conditions<F>(
  conditions: Vec<F>,
  config: RetryConfig,
) -> Result<(), Error>
where
  F: Fn() -> Box<dyn Future<Output = bool> + Send + Unpin>,
{
  poll_until(
    || async {
      for condition in &conditions {
        if !condition().await {
          return false;
        }
      }
      true
    },
    config,
  )
  .await
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::sync::{Arc, Mutex};
  use std::time::Duration;

  #[tokio::test]
  async fn test_retry_with_backoff_success() {
    let counter = Arc::new(Mutex::new(0));
    let counter_clone = counter.clone();

    let result = retry_with_backoff(
      move || {
        let counter = counter_clone.clone();
        async move {
          let mut count = counter.lock().unwrap();
          *count += 1;
          if *count >= 3 {
            Ok("success")
          } else {
            Err(anyhow!("not ready"))
          }
        }
      },
      RetryConfig {
        timeout: Duration::from_secs(5),
        poll_interval: Duration::from_millis(10),
        max_retries: 5,
      },
    )
    .await;

    assert!(result.is_ok());
    assert_eq!(*counter.lock().unwrap(), 3);
  }

  #[tokio::test]
  async fn test_poll_until_some() {
    let counter = Arc::new(Mutex::new(0));
    let counter_clone = counter.clone();

    let result = poll_until_some(
      move || {
        let counter = counter_clone.clone();
        async move {
          let mut count = counter.lock().unwrap();
          *count += 1;
          if *count >= 2 {
            Some("found")
          } else {
            None
          }
        }
      },
      RetryConfig {
        timeout: Duration::from_secs(5),
        poll_interval: Duration::from_millis(10),
        max_retries: 5,
      },
    )
    .await;

    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "found");
  }
}
