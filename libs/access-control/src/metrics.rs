use prometheus_client::metrics::gauge::Gauge;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use prometheus_client::registry::Registry;
use tokio::time::interval;

pub const ENFORCER_METRICS_TICK_INTERVAL: Duration = Duration::from_secs(120);

#[derive(Clone)]
pub struct AccessControlMetrics {
  load_all_policies: Gauge,
  total_read_enforce_count: Gauge,
  read_enforce_from_cache_count: Gauge,
  policy_send_attempts: Gauge,
  policy_send_failures: Gauge,
  policy_channel_full_events: Gauge,
}

impl AccessControlMetrics {
  pub(crate) fn init() -> Self {
    Self {
      load_all_policies: Gauge::default(),
      total_read_enforce_count: Gauge::default(),
      read_enforce_from_cache_count: Gauge::default(),
      policy_send_attempts: Gauge::default(),
      policy_send_failures: Gauge::default(),
      policy_channel_full_events: Gauge::default(),
    }
  }

  pub fn register(registry: &mut Registry) -> Self {
    let metrics = Self::init();
    let realtime_registry = registry.sub_registry_with_prefix("ac");
    realtime_registry.register(
      "load_all_polices",
      "load all polices when server start duration in milliseconds",
      metrics.load_all_policies.clone(),
    );

    realtime_registry.register(
      "total_read_enforce_count",
      "total read enforce count",
      metrics.total_read_enforce_count.clone(),
    );

    realtime_registry.register(
      "read_enforce_from_cache_count",
      "read enforce result from cache",
      metrics.read_enforce_from_cache_count.clone(),
    );

    realtime_registry.register(
      "policy_send_attempts",
      "total policy channel send attempts",
      metrics.policy_send_attempts.clone(),
    );

    realtime_registry.register(
      "policy_send_failures",
      "policy channel send failures (timeout or closed)",
      metrics.policy_send_failures.clone(),
    );

    realtime_registry.register(
      "policy_channel_full_events",
      "times policy channel was full (would block)",
      metrics.policy_channel_full_events.clone(),
    );

    metrics
  }

  pub fn record_load_all_policies_in_ms(&self, millis: u64) {
    self.load_all_policies.set(millis as i64);
  }

  pub fn record_enforce_count(&self, total: i64, from_cache: i64) {
    self.total_read_enforce_count.set(total);
    self.read_enforce_from_cache_count.set(from_cache);
  }

  pub fn record_policy_send_metrics(&self, attempts: i64, failures: i64, channel_full: i64) {
    self.policy_send_attempts.set(attempts);
    self.policy_send_failures.set(failures);
    self.policy_channel_full_events.set(channel_full);
  }
}

#[derive(Clone)]
pub(crate) struct MetricsCalState {
  pub(crate) total_read_enforce_result: Arc<AtomicI64>,
  pub(crate) read_enforce_result_from_cache: Arc<AtomicI64>,
  pub(crate) policy_send_attempts: Arc<AtomicI64>,
  pub(crate) policy_send_failures: Arc<AtomicI64>,
  pub(crate) policy_channel_full_events: Arc<AtomicI64>,
}

impl MetricsCalState {
  pub(crate) fn new() -> Self {
    Self {
      total_read_enforce_result: Arc::new(Default::default()),
      read_enforce_result_from_cache: Arc::new(Default::default()),
      policy_send_attempts: Arc::new(Default::default()),
      policy_send_failures: Arc::new(Default::default()),
      policy_channel_full_events: Arc::new(Default::default()),
    }
  }
}

/// Collect and record metrics for access control
pub(crate) fn tick_metric(state: MetricsCalState, metrics: Arc<AccessControlMetrics>) {
  tokio::spawn(async move {
    let mut interval = interval(ENFORCER_METRICS_TICK_INTERVAL);
    loop {
      interval.tick().await;

      metrics.record_enforce_count(
        state.total_read_enforce_result.load(Ordering::Relaxed),
        state.read_enforce_result_from_cache.load(Ordering::Relaxed),
      );

      metrics.record_policy_send_metrics(
        state.policy_send_attempts.load(Ordering::Relaxed),
        state.policy_send_failures.load(Ordering::Relaxed),
        state.policy_channel_full_events.load(Ordering::Relaxed),
      );
    }
  });
}
