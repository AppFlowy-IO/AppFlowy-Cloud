use prometheus_client::metrics::gauge::Gauge;
use prometheus_client::registry::Registry;
use std::sync::atomic::AtomicU64;


#[derive(Clone)]
pub struct CollabMetrics {
  success_write_snapshot_ratio: Gauge<f64, AtomicU64>,
}

impl CollabMetrics {
  fn init() -> Self {
    Self {
      success_write_snapshot_ratio: Gauge::default(),
    }
  }

  pub fn register(registry: &mut Registry) -> Self {
    let metrics = Self::init();
    let realtime_registry = registry.sub_registry_with_prefix("collab");
    realtime_registry.register(
      "write_snapshot_ratio",
      "success write snapshot ratio",
      metrics.success_write_snapshot_ratio.clone(),
    );

    metrics
  }

  pub fn record_success_write_snapshot_ratio(&self, ratio: f64) {
    self.success_write_snapshot_ratio.set(ratio);
  }
}
