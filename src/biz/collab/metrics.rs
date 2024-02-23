use prometheus_client::metrics::gauge::Gauge;
use prometheus_client::registry::Registry;

#[derive(Clone)]
pub struct CollabMetrics {
  success_write_snapshot_count: Gauge,
  total_write_snapshot_count: Gauge,
}

impl CollabMetrics {
  fn init() -> Self {
    Self {
      success_write_snapshot_count: Gauge::default(),
      total_write_snapshot_count: Default::default(),
    }
  }

  pub fn register(registry: &mut Registry) -> Self {
    let metrics = Self::init();
    let realtime_registry = registry.sub_registry_with_prefix("collab");
    realtime_registry.register(
      "success_write_snapshot_count",
      "success write snapshot to db",
      metrics.success_write_snapshot_count.clone(),
    );
    realtime_registry.register(
      "total_write_snapshot_count",
      "Fail to write snapshot to db",
      metrics.total_write_snapshot_count.clone(),
    );

    metrics
  }

  pub fn record_write_snapshot(&self, success: i64, total: i64) {
    self.success_write_snapshot_count.set(success);
    self.total_write_snapshot_count.set(total);
  }
}
