use prometheus_client::metrics::gauge::Gauge;

use prometheus_client::registry::Registry;
use tracing::trace;

#[derive(Clone)]
pub struct AccessControlMetrics {
  load_all_policies: Gauge,
  total_read_enforce_count: Gauge,
  read_enforce_from_cache_count: Gauge,
}

impl AccessControlMetrics {
  fn init() -> Self {
    Self {
      load_all_policies: Gauge::default(),
      total_read_enforce_count: Gauge::default(),
      read_enforce_from_cache_count: Gauge::default(),
    }
  }

  pub fn register(registry: &mut Registry) -> Self {
    let metrics = Self::init();
    let realtime_registry = registry.sub_registry_with_prefix("ac");
    realtime_registry.register(
      "load_all_polices",
      "load all polices when server start duration in seconds",
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

    metrics
  }

  pub fn record_load_all_policies_in_secs(&self, millis: u64) {
    self.load_all_policies.set(millis as i64);
  }

  pub fn record_enforce_count(&self, total: i64, from_cache: i64) {
    trace!(
      "enforce_count: total: {}, from_cache: {}",
      total,
      from_cache
    );
    self.total_read_enforce_count.set(total);
    self.read_enforce_from_cache_count.set(from_cache);
  }
}
