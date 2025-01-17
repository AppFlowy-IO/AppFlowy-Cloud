use prometheus_client::metrics::counter::Counter;

#[derive(Default, Clone)]
pub struct AIMetrics {
  total_stream_count: Counter,
  failed_stream_count: Counter,
  stream_image_count: Counter,
}

impl AIMetrics {
  pub fn register(registry: &mut prometheus_client::registry::Registry) -> Self {
    let metrics = Self::default();
    let realtime_registry = registry.sub_registry_with_prefix("ai");

    // Register each metric with the Prometheus registry
    realtime_registry.register(
      "total_stream_count",
      "Total count of streams processed",
      metrics.total_stream_count.clone(),
    );
    realtime_registry.register(
      "failed_stream_count",
      "Total count of failed streams",
      metrics.failed_stream_count.clone(),
    );
    realtime_registry.register(
      "image_stream_count",
      "Total count of image streams processed",
      metrics.stream_image_count.clone(),
    );

    metrics
  }

  pub fn record_total_stream_count(&self, count: u64) {
    self.total_stream_count.inc_by(count);
  }

  pub fn record_failed_stream_count(&self, count: u64) {
    self.failed_stream_count.inc_by(count);
  }

  pub fn record_stream_image_count(&self, count: u64) {
    self.stream_image_count.inc_by(count);
  }
}
