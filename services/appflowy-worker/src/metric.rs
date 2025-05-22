use prometheus_client::metrics::gauge::Gauge;
use prometheus_client::metrics::histogram::{Histogram, exponential_buckets};
use prometheus_client::registry::Registry;

pub struct ImportMetrics {
  pub update_size_bytes: Histogram,
  pub import_success_count: Gauge,
  pub import_fail_count: Gauge,
}

impl ImportMetrics {
  pub fn init() -> Self {
    let update_size_buckets = exponential_buckets(1024.0, 2.0, 10);
    Self {
      update_size_bytes: Histogram::new(update_size_buckets),
      import_success_count: Default::default(),
      import_fail_count: Default::default(),
    }
  }

  pub fn register(registry: &mut Registry) -> Self {
    let metrics = Self::init();
    let web_update_registry = registry.sub_registry_with_prefix("appflowy_web");
    web_update_registry.register(
      "import_payload_size_bytes",
      "Size of the update in bytes",
      metrics.update_size_bytes.clone(),
    );
    web_update_registry.register(
      "import_success_count",
      "import success count",
      metrics.import_success_count.clone(),
    );
    web_update_registry.register(
      "import_fail_count",
      "import fail count",
      metrics.import_fail_count.clone(),
    );
    metrics
  }

  pub fn record_import_size_bytes(&self, size: usize) {
    self.update_size_bytes.observe(size as f64);
  }

  pub fn incr_import_success_count(&self, count: i64) {
    self.import_success_count.inc_by(count);
  }

  pub fn incr_import_fail_count(&self, count: i64) {
    self.import_fail_count.inc_by(count);
  }
}
