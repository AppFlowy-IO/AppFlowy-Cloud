use prometheus_client::metrics::gauge::Gauge;
use prometheus_client::registry::Registry;
use tracing::trace;

#[derive(Clone)]
pub struct RealtimeMetrics {
  connected_users: Gauge,
  mem_cache_usage: Gauge,
  opening_collab_count: Gauge,
}

impl RealtimeMetrics {
  fn init() -> Self {
    Self {
      connected_users: Gauge::default(),
      mem_cache_usage: Gauge::default(),
      opening_collab_count: Gauge::default(),
    }
  }

  pub fn register(registry: &mut Registry) -> Self {
    let metrics = Self::init();
    let realtime_registry = registry.sub_registry_with_prefix("realtime");
    realtime_registry.register(
      "connected_users",
      "number of connected users",
      metrics.connected_users.clone(),
    );
    realtime_registry.register(
      "mem_cache_usage",
      "memory cache usage in bytes",
      metrics.mem_cache_usage.clone(),
    );
    realtime_registry.register(
      "opening_collab_count",
      "number of opening collabs",
      metrics.opening_collab_count.clone(),
    );

    metrics
  }

  pub fn record_connected_users(&self, num: usize) {
    trace!("[metrics]: connected_users: {}", num);
    self.connected_users.set(num as i64);
  }

  pub fn record_mem_cache_usage(&self, size_in_bytes: usize) {
    let size_in_mb = size_in_bytes / 1024;
    trace!("[metrics]: mem_cache_usage: {} KB", size_in_mb);
    self.mem_cache_usage.set(size_in_mb as i64);
  }

  pub fn record_opening_collab_count(&self, count: usize) {
    trace!("[metrics]: opening_collab_count: {}", count);
    self.opening_collab_count.set(count as i64);
  }
}
