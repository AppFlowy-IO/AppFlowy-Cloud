use assert_json_diff::CompareMode;
use std::time::Duration;

/// Configuration constants for the test client
pub struct TestClientConstants;

impl TestClientConstants {
  pub const DEFAULT_TIMEOUT_SECS: u64 = 30;
  pub const DEFAULT_POLL_INTERVAL_MS: u64 = 1000;
  pub const MAX_RETRY_COUNT: u32 = 10;
  pub const SYNC_TIMEOUT_SECS: u64 = 60;
  pub const EMBEDDING_TIMEOUT_SECS: u64 = 30;
  pub const EMBEDDING_POLL_INTERVAL_MS: u64 = 2000;
  pub const SEARCH_POLL_INTERVAL_MS: u64 = 1500;
  pub const SNAPSHOT_POLL_INTERVAL_SECS: u64 = 5;
}

/// Configuration for retry operations
#[derive(Clone, Debug)]
pub struct RetryConfig {
  pub timeout: Duration,
  pub poll_interval: Duration,
  pub max_retries: u32,
}

impl Default for RetryConfig {
  fn default() -> Self {
    Self {
      timeout: Duration::from_secs(TestClientConstants::DEFAULT_TIMEOUT_SECS),
      poll_interval: Duration::from_millis(TestClientConstants::DEFAULT_POLL_INTERVAL_MS),
      max_retries: TestClientConstants::MAX_RETRY_COUNT,
    }
  }
}

impl RetryConfig {
  pub fn for_embedding() -> Self {
    Self {
      timeout: Duration::from_secs(TestClientConstants::EMBEDDING_TIMEOUT_SECS),
      poll_interval: Duration::from_millis(TestClientConstants::EMBEDDING_POLL_INTERVAL_MS),
      max_retries: 15,
    }
  }

  pub fn for_sync() -> Self {
    Self {
      timeout: Duration::from_secs(TestClientConstants::SYNC_TIMEOUT_SECS),
      poll_interval: Duration::from_millis(TestClientConstants::DEFAULT_POLL_INTERVAL_MS),
      max_retries: 60,
    }
  }

  pub fn for_search() -> Self {
    Self {
      timeout: Duration::from_secs(TestClientConstants::DEFAULT_TIMEOUT_SECS),
      poll_interval: Duration::from_millis(TestClientConstants::SEARCH_POLL_INTERVAL_MS),
      max_retries: 20,
    }
  }
}

/// Configuration for JSON assertions
#[derive(Clone, Debug)]
pub struct AssertionConfig {
  pub timeout: Duration,
  pub retry_interval: Duration,
  pub comparison_mode: CompareMode,
  pub max_retries: u32,
}

impl Default for AssertionConfig {
  fn default() -> Self {
    Self {
      timeout: Duration::from_secs(TestClientConstants::DEFAULT_TIMEOUT_SECS),
      retry_interval: Duration::from_millis(TestClientConstants::DEFAULT_POLL_INTERVAL_MS),
      comparison_mode: CompareMode::Inclusive,
      max_retries: TestClientConstants::MAX_RETRY_COUNT,
    }
  }
}
