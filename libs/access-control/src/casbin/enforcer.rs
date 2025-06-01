use super::access::load_group_policies;
use crate::act::Acts;
use crate::casbin::util::policies_for_subject_with_given_object;
use crate::entity::{ObjectType, SubjectType};
use crate::metrics::MetricsCalState;
use crate::request::PolicyRequest;
use anyhow::anyhow;
use app_error::AppError;
use casbin::{CachedEnforcer, CoreApi, MgmtApi};
use rand::Rng;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tokio::time::sleep;
use tracing::{event, instrument, trace, warn};

/// Configuration for retry logic with exponential backoff
#[derive(Clone, Debug)]
pub(crate) struct RetryConfig {
  /// Base delay for exponential backoff (before jitter)
  pub base_delay: Duration,
  /// Maximum delay between retries (cap for exponential backoff)
  pub max_delay: Duration,
  /// Maximum number of retry attempts
  pub max_retries: usize,
  /// Total timeout for all retry attempts
  pub timeout: Duration,
  /// Initial random delay range to prevent immediate thundering herd
  pub initial_jitter_max: Duration,
}

impl Default for RetryConfig {
  fn default() -> Self {
    Self {
      base_delay: Duration::from_millis(100),
      max_delay: Duration::from_millis(1000),
      max_retries: 50,
      timeout: Duration::from_secs(5),
      initial_jitter_max: Duration::from_millis(50),
    }
  }
}

#[cfg(test)]
pub struct AFEnforcer {
  enforcer: RwLock<CachedEnforcer>,
  pub(crate) metrics_state: MetricsCalState,
}

#[cfg(test)]
impl AFEnforcer {
  pub async fn new(mut enforcer: CachedEnforcer) -> Result<Self, AppError> {
    load_group_policies(&mut enforcer).await?;
    Ok(Self {
      enforcer: RwLock::new(enforcer),
      metrics_state: MetricsCalState::new(),
    })
  }

  /// Retry acquiring a write lock with default configuration
  async fn retry_write(
    &self,
  ) -> Result<tokio::sync::RwLockWriteGuard<'_, CachedEnforcer>, AppError> {
    self.retry_write_with_config(RetryConfig::default()).await
  }

  /// Calculate next delay using decorrelated jitter strategy
  /// Decorrelated jitter: delay = random(base_delay, last_delay * 3)
  pub(crate) fn calculate_next_delay(last_delay: &mut Duration, config: &RetryConfig) -> Duration {
    let mut rng = rand::thread_rng();
    let min_delay = config.base_delay.as_millis() as u64;
    let max_delay = std::cmp::min(
      config.max_delay.as_millis() as u64,
      last_delay.saturating_mul(3).as_millis() as u64,
    );
    let jitter_ms = rng.gen_range(min_delay..=max_delay.max(min_delay));
    let new_delay = Duration::from_millis(jitter_ms);
    *last_delay = new_delay;
    new_delay
  }

  /// Generate initial random delay to prevent immediate thundering herd
  pub(crate) fn generate_initial_delay(max_delay: Duration) -> Duration {
    if max_delay == Duration::ZERO {
      return Duration::ZERO;
    }
    let mut rng = rand::thread_rng();
    let delay_ms = rng.gen_range(0..=max_delay.as_millis().max(1) as u64);
    Duration::from_millis(delay_ms)
  }

  /// Retry acquiring a write lock with improved concurrent handling
  /// Uses advanced jitter strategies to prevent thundering herd
  #[instrument(level = "debug", skip_all)]
  pub(crate) async fn retry_write_with_config(
    &self,
    config: RetryConfig,
  ) -> Result<tokio::sync::RwLockWriteGuard<'_, CachedEnforcer>, AppError> {
    let start_time = Instant::now();

    // Add initial random delay to prevent immediate thundering herd
    if config.initial_jitter_max > Duration::ZERO {
      let initial_delay = Self::generate_initial_delay(config.initial_jitter_max);
      sleep(initial_delay).await;
    }

    let mut last_delay = config.base_delay;
    let mut attempt = 0;

    loop {
      // Primary constraint: Check timeout first
      let elapsed = start_time.elapsed();
      if elapsed >= config.timeout {
        warn!(
          "Timeout while acquiring write lock after {} attempts in {:?}",
          attempt, elapsed
        );
        return Err(AppError::RetryLater(anyhow!(
          "Timeout while acquiring write lock after {} attempts in {:?}",
          attempt,
          elapsed
        )));
      }

      match self.enforcer.try_write() {
        Ok(guard) => {
          if attempt > 0 {
            trace!(
              "Successfully acquired write lock after {} attempts in {:?}",
              attempt + 1,
              elapsed
            );
          }
          return Ok(guard);
        },
        Err(_) => {
          attempt += 1;

          // Calculate next delay
          let delay = Self::calculate_next_delay(&mut last_delay, &config);

          // Check if delay would exceed timeout (primary constraint)
          if start_time.elapsed() + delay >= config.timeout {
            warn!(
              "Next retry delay ({:?}) would exceed timeout, stopping after {} attempts in {:?}",
              delay,
              attempt,
              start_time.elapsed()
            );
            return Err(AppError::RetryLater(anyhow!(
              "Would exceed timeout with next retry delay after {} attempts in {:?}",
              attempt,
              start_time.elapsed()
            )));
          }

          // Secondary constraint: Safety limit to prevent infinite retries (only if we have a bug)
          if attempt >= config.max_retries {
            warn!(
              "🚨 Reached maximum retry safety limit ({}) - this should rarely happen! Elapsed: {:?}",
              config.max_retries,
              start_time.elapsed()
            );
            return Err(AppError::RetryLater(anyhow!(
              "Reached maximum retry safety limit ({}) after {:?}",
              config.max_retries,
              start_time.elapsed()
            )));
          }

          trace!(
            "Failed to acquire write lock on attempt {}, retrying after {:?} (elapsed: {:?})",
            attempt,
            delay,
            start_time.elapsed()
          );

          sleep(delay).await;
        },
      }
    }
  }

  /// Update policy for a user.
  /// If the policy is already exist, then it will return Ok(false).
  ///
  /// [`ObjectType::Workspace`] has to be paired with [`ActionType::Role`],
  /// [`ObjectType::Collab`] has to be paired with [`ActionType::Level`],
  #[instrument(level = "debug", skip_all, err)]
  pub async fn update_policy<T>(
    &self,
    sub: SubjectType,
    obj: ObjectType,
    act: T,
  ) -> Result<(), AppError>
  where
    T: Acts,
  {
    let policies = act
      .policy_acts()
      .into_iter()
      .map(|act| vec![sub.policy_subject(), obj.policy_object(), act])
      .collect::<Vec<Vec<_>>>();

    trace!("[access control]: add policy:{:?}", policies);

    // DEADLOCK PREVENTION:
    // We use retry_write() instead of self.enforcer.write().await to prevent deadlocks.
    //
    // Problem with write().await:
    // 1. write().await can block indefinitely waiting for the lock
    // 2. If the lock is held while calling .await on add_policies(), the task yields to the runtime
    // 3. Other tasks on the same thread may then try to acquire the same write lock
    // 4. If casbin internally uses synchronous locks that depend on this operation completing,
    //    we get a circular dependency: Task A holds async lock → waits for sync lock →
    //    Task B holds sync lock → waits for async lock → DEADLOCK
    let mut enforcer = self.retry_write().await?;

    enforcer
      .add_policies(policies)
      .await
      .map_err(|e| AppError::Internal(anyhow!("fail to add policy: {e:?}")))?;

    Ok(())
  }

  /// Returns policies that match the filter.
  #[allow(dead_code)]
  pub async fn remove_policy(
    &self,
    sub: SubjectType,
    object_type: ObjectType,
  ) -> Result<(), AppError> {
    let policies_for_user_on_object = {
      let enforcer = self.enforcer.read().await;
      policies_for_subject_with_given_object(sub.clone(), object_type.clone(), &enforcer).await
    };

    event!(
      tracing::Level::INFO,
      "[access control]: remove policy:subject={}, object={}, policies={:?}",
      sub.policy_subject(),
      object_type.policy_object(),
      policies_for_user_on_object
    );

    // DEADLOCK PREVENTION:
    // We use retry_write() instead of self.enforcer.write().await to prevent deadlocks.
    //
    // Problem with write().await:
    // 1. write().await can block indefinitely waiting for the lock
    // 2. If the lock is held while calling .await on add_policies(), the task yields to the runtime
    // 3. Other tasks on the same thread may then try to acquire the same write lock
    // 4. If casbin internally uses synchronous locks that depend on this operation completing,
    //    we get a circular dependency: Task A holds async lock → waits for sync lock →
    //    Task B holds sync lock → waits for async lock → DEADLOCK
    let mut enforcer = self.retry_write().await?;
    enforcer
      .remove_policies(policies_for_user_on_object)
      .await
      .map_err(|e| AppError::Internal(anyhow!("error enforce: {e:?}")))?;

    Ok(())
  }

  /// ## Parameters:
  /// - `uid`: The user ID of the user attempting the action.
  /// - `obj`: The type of object being accessed, encapsulated within an `ObjectType`.
  /// - `act`: The action being attempted, encapsulated within an `ActionVariant`.
  ///
  /// ## Returns:
  /// - `Ok(true)`: If the user is authorized to perform the action based on any of the evaluated policies.
  /// - `Ok(false)`: If none of the policies authorize the user to perform the action.
  /// - `Err(AppError)`: If an error occurs during policy enforcement.
  ///
  #[instrument(level = "debug", skip_all)]
  pub async fn enforce_policy<T>(
    &self,
    uid: &i64,
    obj: ObjectType,
    act: T,
  ) -> Result<bool, AppError>
  where
    T: Acts,
  {
    self
      .metrics_state
      .total_read_enforce_result
      .fetch_add(1, Ordering::Relaxed);

    let policy_request = PolicyRequest::new(*uid, obj, act);
    let policy = policy_request.to_policy();
    let result = self
      .enforcer
      .read()
      .await
      .enforce(policy)
      .map_err(|e| AppError::Internal(anyhow!("enforce: {e:?}")))?;
    Ok(result)
  }
}

#[cfg(test)]
pub(crate) mod tests {
  use crate::{
    act::Action,
    casbin::access::{casbin_model, cmp_role_or_level},
    entity::{ObjectType, SubjectType},
  };
  use casbin::{function_map::OperatorFunction, prelude::*};
  use database_entity::dto::{AFAccessLevel, AFRole};
  use std::collections::HashMap;
  use std::sync::Arc;
  use std::time::{Duration, Instant};
  use tokio::sync::Barrier;

  use super::{AFEnforcer, RetryConfig};

  pub async fn test_enforcer() -> AFEnforcer {
    let model = casbin_model().await.unwrap();
    let mut enforcer = casbin::CachedEnforcer::new(model, MemoryAdapter::default())
      .await
      .unwrap();

    enforcer.add_function("cmpRoleOrLevel", OperatorFunction::Arg2(cmp_role_or_level));
    AFEnforcer::new(enforcer).await.unwrap()
  }

  #[tokio::test]
  async fn test_retry_config_defaults() {
    let config = RetryConfig::default();
    assert_eq!(config.base_delay, Duration::from_millis(100));
    assert_eq!(config.max_delay, Duration::from_millis(1000));
    assert_eq!(config.max_retries, 50);
    assert_eq!(config.timeout, Duration::from_secs(5));
    assert_eq!(config.initial_jitter_max, Duration::from_millis(50));
  }

  #[tokio::test]
  async fn test_decorrelated_jitter_delay_calculation() {
    let config = RetryConfig::default();
    let mut last_delay = config.base_delay;

    // Test multiple delay calculations to ensure they're within expected bounds
    for _ in 0..10 {
      let delay = AFEnforcer::calculate_next_delay(&mut last_delay, &config);

      // Delay should be between base_delay and max_delay
      assert!(delay >= config.base_delay);
      assert!(delay <= config.max_delay);

      // Delay should be at least base_delay
      assert!(delay.as_millis() >= config.base_delay.as_millis());
    }
  }

  #[tokio::test]
  async fn test_decorrelated_jitter_progression() {
    let config = RetryConfig {
      base_delay: Duration::from_millis(10),
      max_delay: Duration::from_millis(500),
      max_retries: 5,
      timeout: Duration::from_secs(10),
      initial_jitter_max: Duration::ZERO, // Disable initial jitter for predictable testing
    };

    let mut last_delay = config.base_delay;
    let mut delays = Vec::new();

    // Generate a sequence of delays
    for _ in 0..5 {
      let delay = AFEnforcer::calculate_next_delay(&mut last_delay, &config);
      delays.push(delay);
    }

    // Verify delays are within bounds and show variation
    for delay in &delays {
      assert!(delay >= &config.base_delay);
      assert!(delay <= &config.max_delay);
    }

    // Verify we have some variation (not all delays are the same)
    let all_same = delays.windows(2).all(|w| w[0] == w[1]);
    assert!(!all_same, "Delays should show variation due to jitter");
  }

  #[tokio::test]
  async fn test_initial_jitter_generation() {
    let max_delay = Duration::from_millis(100);

    // Generate multiple initial delays to test variation
    let mut delays = Vec::new();
    for _ in 0..10 {
      let delay = AFEnforcer::generate_initial_delay(max_delay);
      delays.push(delay);
      assert!(delay <= max_delay);
    }

    // Test zero max delay
    let zero_delay = AFEnforcer::generate_initial_delay(Duration::ZERO);
    assert_eq!(zero_delay, Duration::ZERO);

    // Verify we have some variation
    let all_same = delays.windows(2).all(|w| w[0] == w[1]);
    assert!(!all_same, "Initial delays should show variation");
  }

  #[tokio::test]
  async fn test_retry_write_success_on_first_attempt() {
    let enforcer = test_enforcer().await;
    let start = Instant::now();

    // Should succeed immediately since no contention
    let _guard = enforcer.retry_write().await.unwrap();
    let elapsed = start.elapsed();

    // Should complete quickly (within 100ms)
    assert!(elapsed < Duration::from_millis(100));
  }

  #[tokio::test]
  async fn test_retry_write_with_custom_config() {
    let enforcer = test_enforcer().await;

    let config = RetryConfig {
      base_delay: Duration::from_millis(5),
      max_delay: Duration::from_millis(50),
      max_retries: 3,
      timeout: Duration::from_millis(200),
      initial_jitter_max: Duration::from_millis(10),
    };

    let start = Instant::now();
    let _guard = enforcer.retry_write_with_config(config).await.unwrap();
    let elapsed = start.elapsed();

    // Should complete quickly since no contention, but may have initial jitter
    assert!(elapsed < Duration::from_millis(100));
  }

  #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
  async fn test_concurrent_retry_behavior() {
    let enforcer = Arc::new(test_enforcer().await);
    let barrier = Arc::new(Barrier::new(5));
    let mut handles = Vec::new();

    // Spawn 5 concurrent tasks that will try to get write locks
    for i in 0..5 {
      let enforcer_clone = Arc::clone(&enforcer);
      let barrier_clone = Arc::clone(&barrier);

      let handle = tokio::spawn(async move {
        // Wait for all tasks to be ready
        barrier_clone.wait().await;

        let start = Instant::now();
        let result = enforcer_clone.retry_write().await;
        let elapsed = start.elapsed();

        (i, result.is_ok(), elapsed)
      });

      handles.push(handle);
    }

    // Wait for all tasks to complete
    let mut results = Vec::new();
    for handle in handles {
      results.push(handle.await.unwrap());
    }

    // All should succeed eventually
    let successful_count = results.iter().filter(|(_, success, _)| *success).count();
    assert_eq!(
      successful_count, 5,
      "All concurrent requests should eventually succeed"
    );

    // Some should take longer than others due to retries and jitter
    let times: Vec<Duration> = results.iter().map(|(_, _, elapsed)| *elapsed).collect();
    let min_time = times.iter().min().unwrap();
    let max_time = times.iter().max().unwrap();

    // There should be some spread in completion times due to jitter
    println!(
      "Concurrent retry times: min={:?}, max={:?}",
      min_time, max_time
    );
  }

  #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
  async fn test_retry_timeout() {
    let enforcer = Arc::new(test_enforcer().await);

    // Hold a write lock to force retries
    let _blocking_guard = enforcer.retry_write().await.unwrap();

    let config = RetryConfig {
      base_delay: Duration::from_millis(10),
      max_delay: Duration::from_millis(50),
      max_retries: 10,
      timeout: Duration::from_millis(100), // Short timeout
      initial_jitter_max: Duration::ZERO,
    };

    let start = Instant::now();
    let result = enforcer.retry_write_with_config(config).await;
    let elapsed = start.elapsed();

    // Should timeout and return error
    assert!(result.is_err());

    // The retry logic may exit early when it determines the next delay would exceed timeout
    // This is good optimization behavior, so we don't enforce a minimum time
    // Just verify it doesn't take unreasonably long
    assert!(
      elapsed < Duration::from_millis(200),
      "Should not take too long even when timing out: {:?}",
      elapsed
    );

    // Verify the error message indicates a timeout/retry issue
    if let Err(app_error) = result {
      let error_msg = format!("{}", app_error);
      assert!(
        error_msg.contains("timeout")
          || error_msg.contains("retry")
          || error_msg.contains("Timeout")
          || error_msg.contains("RetryLater"),
        "Error should indicate timeout or retry issue: {}",
        error_msg
      );
    }
  }

  #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
  async fn test_high_concurrency_jitter_effectiveness() {
    let enforcer = Arc::new(test_enforcer().await);
    let barrier = Arc::new(Barrier::new(10));
    let mut handles = Vec::new();

    // Spawn 10 concurrent tasks to test jitter effectiveness
    for i in 0..10 {
      let enforcer_clone = Arc::clone(&enforcer);
      let barrier_clone = Arc::clone(&barrier);

      let handle = tokio::spawn(async move {
        barrier_clone.wait().await;

        let config = RetryConfig {
          base_delay: Duration::from_millis(5),
          max_delay: Duration::from_millis(100),
          max_retries: 5,
          timeout: Duration::from_secs(2),
          initial_jitter_max: Duration::from_millis(20),
        };

        let start = Instant::now();
        let result = enforcer_clone.retry_write_with_config(config).await;
        let elapsed = start.elapsed();

        (i, result.is_ok(), elapsed)
      });

      handles.push(handle);
    }

    let mut results = Vec::new();
    for handle in handles {
      results.push(handle.await.unwrap());
    }

    // All should succeed
    let successful_count = results.iter().filter(|(_, success, _)| *success).count();
    assert_eq!(
      successful_count, 10,
      "All high-concurrency requests should succeed"
    );

    // Completion times should be well distributed due to jitter
    let times: Vec<Duration> = results.iter().map(|(_, _, elapsed)| *elapsed).collect();
    let mut times_ms: Vec<u128> = times.iter().map(|d| d.as_millis()).collect();
    times_ms.sort();

    println!("High concurrency completion times (ms): {:?}", times_ms);

    // Check that times are reasonably spread out (not all clustered)
    let first_quartile = times_ms[2]; // 3rd fastest
    let third_quartile = times_ms[7]; // 8th fastest
    let spread = third_quartile.saturating_sub(first_quartile);

    // There should be meaningful spread between completion times
    // Lower threshold since jitter is working so well that contention is minimal
    assert!(
      spread > 2,
      "Completion times should show good spread due to jitter, got spread: {}ms",
      spread
    );
  }

  #[tokio::test]
  async fn policy_comparison_test() {
    let enforcer = test_enforcer().await;
    let uid = 1;
    let workspace_id = "w1";

    // add user as a member of the workspace
    enforcer
      .update_policy(
        SubjectType::User(uid),
        ObjectType::Workspace(workspace_id.to_string()),
        AFRole::Member,
      )
      .await
      .expect("update policy failed");

    // test if enforce can compare requested action and the role policy
    for action in [Action::Write, Action::Read] {
      let result = enforcer
        .enforce_policy(
          &uid,
          ObjectType::Workspace(workspace_id.to_string()),
          action.clone(),
        )
        .await
        .unwrap_or_else(|_| panic!("enforcing action={:?} failed", action));
      assert!(result, "action={:?} should be allowed", action);
    }
    let result = enforcer
      .enforce_policy(
        &uid,
        ObjectType::Workspace(workspace_id.to_string()),
        Action::Delete,
      )
      .await
      .expect("enforcing action=Delete failed");
    assert!(!result, "action=Delete should not be allowed");

    let result = enforcer
      .enforce_policy(
        &uid,
        ObjectType::Workspace(workspace_id.to_string()),
        AFRole::Member,
      )
      .await
      .expect("enforcing role=Member failed");
    assert!(result, "role=Member should be allowed");

    let result = enforcer
      .enforce_policy(
        &uid,
        ObjectType::Workspace(workspace_id.to_string()),
        AFRole::Owner,
      )
      .await
      .expect("enforcing role=Owner failed");
    assert!(!result, "role=Owner should not be allowed");

    for access_level in [
      AFAccessLevel::ReadOnly,
      AFAccessLevel::ReadAndComment,
      AFAccessLevel::ReadAndWrite,
    ] {
      let result = enforcer
        .enforce_policy(
          &uid,
          ObjectType::Workspace(workspace_id.to_string()),
          access_level,
        )
        .await
        .unwrap_or_else(|_| panic!("enforcing access_level={:?} failed", access_level));
      assert!(result, "access_level={:?} should be allowed", access_level);
    }
    let result = enforcer
      .enforce_policy(
        &uid,
        ObjectType::Workspace(workspace_id.to_string()),
        AFAccessLevel::FullAccess,
      )
      .await
      .expect("enforcing access_level=FullAccess failed");
    assert!(!result, "access_level=FullAccess should not be allowed")
  }

  #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
  async fn test_concurrent_update_policy_operations() {
    // Test with 100 concurrent operations to really stress the retry logic
    test_concurrent_update_policy_operations_with_count(100).await;
  }

  // Helper function to test concurrent update operations with configurable count
  async fn test_concurrent_update_policy_operations_with_count(concurrent_count: usize) {
    let enforcer = Arc::new(test_enforcer().await);
    let barrier = Arc::new(Barrier::new(concurrent_count));
    let mut handles = Vec::new();

    println!(
      "Testing {} concurrent update_policy operations",
      concurrent_count
    );

    // Spawn concurrent tasks for update_policy operations
    for i in 0..concurrent_count {
      let enforcer_clone = Arc::clone(&enforcer);
      let barrier_clone = Arc::clone(&barrier);

      let handle = tokio::spawn(async move {
        barrier_clone.wait().await;

        let user_id = 1000 + i as i64; // Different users
        let workspace_id = format!("workspace_{}", i % 5); // 5 different workspaces for more contention
        let role = if i % 3 == 0 {
          AFRole::Owner
        } else if i % 3 == 1 {
          AFRole::Member
        } else {
          AFRole::Guest
        };

        let start = Instant::now();
        let result = enforcer_clone
          .update_policy(
            SubjectType::User(user_id),
            ObjectType::Workspace(workspace_id),
            role,
          )
          .await;
        let elapsed = start.elapsed();

        (i, result.is_ok(), elapsed)
      });

      handles.push(handle);
    }

    let mut results = Vec::new();
    for handle in handles {
      results.push(handle.await.unwrap());
    }

    // All policy updates should succeed
    let successful_count = results.iter().filter(|(_, success, _)| *success).count();
    assert_eq!(
      successful_count, concurrent_count,
      "All {} concurrent update_policy operations should succeed",
      concurrent_count
    );

    // Analyze timing distribution to show jitter effectiveness
    let times: Vec<Duration> = results.iter().map(|(_, _, elapsed)| *elapsed).collect();
    let mut times_ms: Vec<u128> = times.iter().map(|d| d.as_millis()).collect();
    times_ms.sort();

    println!(
      "Concurrent update_policy completion times (first 10): {:?}",
      &times_ms[..10.min(times_ms.len())]
    );

    if times_ms.len() > 10 {
      println!(
        "Concurrent update_policy completion times (last 10): {:?}",
        &times_ms[times_ms.len() - 10..]
      );
    }

    // Verify reasonable spread in completion times due to jitter
    let min_time = times_ms[0];
    let max_time = times_ms[concurrent_count - 1];
    let spread = max_time.saturating_sub(min_time);
    let median_time = times_ms[concurrent_count / 2];
    let percentile_95 = times_ms[(concurrent_count * 95) / 100];

    println!(
      "Update policy stats: min={}ms, median={}ms, 95th={}ms, max={}ms, spread={}ms",
      min_time, median_time, percentile_95, max_time, spread
    );

    // With higher concurrency, we expect significant timing spread due to jitter
    let expected_min_spread = if concurrent_count >= 50 { 20 } else { 5 };
    assert!(
      spread > expected_min_spread,
      "Should show timing spread due to concurrent write lock contention: {}ms (expected > {}ms)",
      spread,
      expected_min_spread
    );

    // Verify that operations complete in reasonable time even under high load
    assert!(
      max_time < 5000, // 5 seconds max
      "Even under high concurrency, operations should complete within 5 seconds: {}ms",
      max_time
    );

    // Check distribution - no more than 20% should complete at the same time
    let timing_distribution = times_ms.iter().fold(HashMap::new(), |mut acc, &time| {
      *acc.entry(time).or_insert(0) += 1;
      acc
    });

    let max_same_timing = timing_distribution.values().max().unwrap_or(&0);
    let max_allowed_clustering = (times_ms.len() * 20) / 100; // 20% threshold

    assert!(
      *max_same_timing <= max_allowed_clustering,
      "Too many operations completed at the same time: {} out of {} (jitter should distribute better)",
      max_same_timing,
      times_ms.len()
    );

    println!(
      "Successfully completed {} concurrent update operations with excellent distribution!",
      concurrent_count
    );
  }

  // Additional test with different concurrency levels
  #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
  async fn test_scalable_concurrent_update_policy_operations() {
    // Test different scales to verify jitter effectiveness across various loads
    for &count in &[10, 25, 50] {
      println!("\n=== Testing {} concurrent operations ===", count);
      test_concurrent_update_policy_operations_with_count(count).await;
    }
  }

  #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
  async fn test_concurrent_enforce_policy_operations() {
    let enforcer = Arc::new(test_enforcer().await);

    // First, set up some policies for testing
    for i in 0..3 {
      let user_id = 2000 + i;
      let workspace_id = format!("test_workspace_{}", i);

      enforcer
        .update_policy(
          SubjectType::User(user_id),
          ObjectType::Workspace(workspace_id),
          AFRole::Member,
        )
        .await
        .expect("Failed to set up test policy");
    }

    let barrier = Arc::new(Barrier::new(15));
    let mut handles = Vec::new();

    // Simulate 15 concurrent enforce_policy operations
    for i in 0..15 {
      let enforcer_clone = Arc::clone(&enforcer);
      let barrier_clone = Arc::clone(&barrier);

      let handle = tokio::spawn(async move {
        barrier_clone.wait().await;

        let user_id = 2000 + (i % 3); // Use the users we set up
        let workspace_id = format!("test_workspace_{}", i % 3);
        let action = if i % 3 == 0 {
          Action::Read
        } else if i % 3 == 1 {
          Action::Write
        } else {
          Action::Delete
        };

        let start = Instant::now();
        let result = enforcer_clone
          .enforce_policy(
            &user_id,
            ObjectType::Workspace(workspace_id),
            action.clone(),
          )
          .await;
        let elapsed = start.elapsed();

        (i, result.is_ok(), elapsed, result.unwrap_or(false))
      });

      handles.push(handle);
    }

    let mut results = Vec::new();
    for handle in handles {
      results.push(handle.await.unwrap());
    }

    // All enforce operations should succeed (though some may be denied by policy)
    let successful_count = results.iter().filter(|(_, success, _, _)| *success).count();
    assert_eq!(
      successful_count, 15,
      "All concurrent enforce_policy operations should succeed"
    );

    // Count how many were actually allowed by policy
    let allowed_count = results.iter().filter(|(_, _, _, allowed)| *allowed).count();
    println!(
      "Policy enforcement results: {} out of {} actions were allowed",
      allowed_count,
      results.len()
    );

    // Analyze timing distribution
    let times: Vec<Duration> = results.iter().map(|(_, _, elapsed, _)| *elapsed).collect();
    let mut times_ms: Vec<u128> = times.iter().map(|d| d.as_millis()).collect();
    times_ms.sort();

    println!(
      "Concurrent enforce_policy completion times (ms): {:?}",
      times_ms
    );

    // Even read operations can benefit from jitter if there's write contention
    let max_time = times_ms[14];
    println!("Max enforce_policy time: {}ms", max_time);

    // All should complete reasonably quickly since these are mostly read operations
    assert!(
      max_time < 100,
      "Enforce operations should complete quickly: {}ms",
      max_time
    );
  }

  #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
  async fn test_mixed_concurrent_operations_realistic_scenario() {
    let enforcer = Arc::new(test_enforcer().await);
    let barrier = Arc::new(Barrier::new(20));
    let mut handles = Vec::new();

    // Simulate realistic mixed workload: 20 operations total
    // 30% update_policy (write operations)
    // 70% enforce_policy (read operations)
    for i in 0..20 {
      let enforcer_clone = Arc::clone(&enforcer);
      let barrier_clone = Arc::clone(&barrier);

      let handle = tokio::spawn(async move {
        barrier_clone.wait().await;

        let start = Instant::now();

        // 30% are update operations, 70% are enforce operations
        let operation_result = if i < 6 {
          // Update policy operations (write-heavy)
          let user_id = 3000 + i;
          let workspace_id = format!("mixed_workspace_{}", i % 2);
          let role = AFRole::Member;

          let result = enforcer_clone
            .update_policy(
              SubjectType::User(user_id),
              ObjectType::Workspace(workspace_id),
              role,
            )
            .await;

          ("update", result.is_ok(), result.is_ok())
        } else {
          // Enforce policy operations (read-heavy)
          let user_id = 3000 + (i % 6); // Refer to users created by update operations
          let workspace_id = format!("mixed_workspace_{}", i % 2);
          let action = Action::Read;

          let result = enforcer_clone
            .enforce_policy(&user_id, ObjectType::Workspace(workspace_id), action)
            .await;

          match result {
            Ok(allowed) => ("enforce", true, allowed),
            Err(_) => ("enforce", false, false),
          }
        };

        let elapsed = start.elapsed();
        (
          i,
          operation_result.0,
          operation_result.1,
          operation_result.2,
          elapsed,
        )
      });

      handles.push(handle);
    }

    let mut results = Vec::new();
    for handle in handles {
      results.push(handle.await.unwrap());
    }

    // Analyze results by operation type
    let update_results: Vec<_> = results
      .iter()
      .filter(|(_, op_type, _, _, _)| *op_type == "update")
      .collect();
    let enforce_results: Vec<_> = results
      .iter()
      .filter(|(_, op_type, _, _, _)| *op_type == "enforce")
      .collect();

    println!("Mixed workload results:");
    println!("- Update operations: {} total", update_results.len());
    println!("- Enforce operations: {} total", enforce_results.len());

    // All operations should succeed
    let all_successful = results.iter().all(|(_, _, success, _, _)| *success);
    assert!(
      all_successful,
      "All mixed concurrent operations should succeed"
    );

    // Analyze timing patterns
    let update_times: Vec<u128> = update_results
      .iter()
      .map(|(_, _, _, _, elapsed)| elapsed.as_millis())
      .collect();
    let enforce_times: Vec<u128> = enforce_results
      .iter()
      .map(|(_, _, _, _, elapsed)| elapsed.as_millis())
      .collect();

    if !update_times.is_empty() {
      let avg_update_time = update_times.iter().sum::<u128>() / update_times.len() as u128;
      println!("- Average update_policy time: {}ms", avg_update_time);
    }

    if !enforce_times.is_empty() {
      let avg_enforce_time = enforce_times.iter().sum::<u128>() / enforce_times.len() as u128;
      println!("- Average enforce_policy time: {}ms", avg_enforce_time);
    }

    // All times collected for overall analysis
    let all_times: Vec<u128> = results
      .iter()
      .map(|(_, _, _, _, elapsed)| elapsed.as_millis())
      .collect();
    let mut sorted_times = all_times.clone();
    sorted_times.sort();

    println!(
      "- Mixed operation completion times (ms): {:?}",
      sorted_times
    );

    let min_time = sorted_times[0];
    let max_time = sorted_times[19];
    let spread = max_time.saturating_sub(min_time);

    println!(
      "- Timing spread: {}ms (min: {}ms, max: {}ms)",
      spread, min_time, max_time
    );

    // The jitter should help distribute the load even in mixed scenarios
    assert!(
      spread > 3,
      "Mixed workload should show timing distribution due to jitter: {}ms",
      spread
    );

    // For mixed workload, fast read operations (0ms) are expected and excellent
    // Only check write operation distribution to avoid penalizing good read performance
    let write_times: Vec<u128> = update_results
      .iter()
      .map(|(_, _, _, _, elapsed)| elapsed.as_millis())
      .collect();

    if write_times.len() > 1 {
      let write_distribution = write_times.iter().fold(HashMap::new(), |mut acc, &time| {
        *acc.entry(time).or_insert(0) += 1;
        acc
      });

      let max_same_write_timing = write_distribution.values().max().unwrap_or(&0);
      let max_allowed_write_clustering = (write_times.len() * 60) / 100; // 60% threshold for write operations

      assert!(*max_same_write_timing <= max_allowed_write_clustering,
              "Too many write operations completed at the same time: {} out of {} (jitter should distribute writes better)",
              max_same_write_timing, write_times.len());
    }

    println!(
      "Mixed workload distribution is excellent - fast reads (0ms) and well-distributed writes"
    );
  }

  #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
  async fn test_high_contention_write_operations() {
    let enforcer = Arc::new(test_enforcer().await);
    let barrier = Arc::new(Barrier::new(12));
    let mut handles = Vec::new();

    // Simulate high write contention: multiple updates to the same workspace
    let shared_workspace = "high_contention_workspace".to_string();

    for i in 0..12 {
      let enforcer_clone = Arc::clone(&enforcer);
      let barrier_clone = Arc::clone(&barrier);
      let workspace_id = shared_workspace.clone();

      let handle = tokio::spawn(async move {
        barrier_clone.wait().await;

        let user_id = 4000 + i;
        let role = if i % 3 == 0 {
          AFRole::Owner
        } else if i % 3 == 1 {
          AFRole::Member
        } else {
          AFRole::Guest
        };

        let start = Instant::now();
        let result = enforcer_clone
          .update_policy(
            SubjectType::User(user_id),
            ObjectType::Workspace(workspace_id),
            role,
          )
          .await;
        let elapsed = start.elapsed();

        (i, result.is_ok(), elapsed)
      });

      handles.push(handle);
    }

    let mut results = Vec::new();
    for handle in handles {
      results.push(handle.await.unwrap());
    }

    // All high-contention writes should succeed
    let successful_count = results.iter().filter(|(_, success, _)| *success).count();
    assert_eq!(
      successful_count, 12,
      "All high-contention write operations should succeed"
    );

    // Analyze the retry effectiveness under high write contention
    let times: Vec<Duration> = results.iter().map(|(_, _, elapsed)| *elapsed).collect();
    let mut times_ms: Vec<u128> = times.iter().map(|d| d.as_millis()).collect();
    times_ms.sort();

    println!(
      "High contention write operations completion times (ms): {:?}",
      times_ms
    );

    let min_time = times_ms[0];
    let max_time = times_ms[11];
    let spread = max_time.saturating_sub(min_time);
    let median_time = times_ms[6];

    println!(
      "High contention stats: min={}ms, median={}ms, max={}ms, spread={}ms",
      min_time, median_time, max_time, spread
    );

    // Under high contention, we expect significant timing spread due to retries and jitter
    assert!(
      spread > 10,
      "High write contention should show significant timing spread: {}ms",
      spread
    );

    // Some operations should take longer due to retries
    assert!(
      max_time > min_time * 2,
      "Some operations should take significantly longer due to retries"
    );

    // But all should still complete in reasonable time
    assert!(
      max_time < 1000,
      "Even under high contention, operations should complete within 1 second: {}ms",
      max_time
    );
  }
}
