use prometheus_client::metrics::gauge::Gauge;
use prometheus_client::registry::Registry;
use std::sync::atomic::AtomicU64;

#[derive(Clone)]
pub struct RealtimeMetrics {
  connected_users: Gauge,
  encode_collab_mem_hit_rate: Gauge<f64, AtomicU64>,
  opening_collab_count: Gauge,
}

impl RealtimeMetrics {
  fn init() -> Self {
    Self {
      connected_users: Gauge::default(),
      encode_collab_mem_hit_rate: Gauge::default(),
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
      "mem_hit_rate",
      "memory hit rate",
      metrics.encode_collab_mem_hit_rate.clone(),
    );
    realtime_registry.register(
      "opening_collab_count",
      "number of opening collabs",
      metrics.opening_collab_count.clone(),
    );

    metrics
  }

  pub fn record_connected_users(&self, num: usize) {
    self.connected_users.set(num as i64);
  }

  pub fn record_encode_collab_mem_hit_rate(&self, rate: f64) {
    self.encode_collab_mem_hit_rate.set(rate);
  }

  pub fn record_opening_collab_count(&self, count: usize) {
    self.opening_collab_count.set(count as i64);
  }
}
