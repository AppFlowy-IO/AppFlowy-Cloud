use prometheus_client::metrics::{counter::Counter, histogram::Histogram};
use prometheus_client::registry::Registry;
#[derive(Clone)]
pub struct EmbeddingMetrics {
  total_embed_count: Counter,
  failed_embed_count: Counter,
  processing_time_histogram: Histogram,
}

impl EmbeddingMetrics {
  fn init() -> Self {
    Self {
      total_embed_count: Counter::default(),
      failed_embed_count: Counter::default(),
      processing_time_histogram: Histogram::new([100.0, 300.0, 800.0, 2000.0, 5000.0].into_iter()),
    }
  }

  pub fn register(registry: &mut Registry) -> Self {
    let metrics = Self::init();
    let realtime_registry = registry.sub_registry_with_prefix("embedding");

    // Register each metric with the Prometheus registry
    realtime_registry.register(
      "total_embed_count",
      "Total count of embeddings processed",
      metrics.total_embed_count.clone(),
    );
    realtime_registry.register(
      "failed_embed_count",
      "Total count of failed embeddings",
      metrics.failed_embed_count.clone(),
    );
    realtime_registry.register(
      "processing_time_seconds",
      "Histogram of embedding processing times",
      metrics.processing_time_histogram.clone(),
    );

    metrics
  }

  pub fn record_embed_count(&self, count: u64) {
    self.total_embed_count.inc_by(count);
  }

  pub fn record_failed_embed_count(&self, count: u64) {
    self.failed_embed_count.inc_by(count);
  }

  pub fn record_processing_time(&self, millis: u128) {
    tracing::trace!("[Embedding]: processing time: {}ms", millis);
    self.processing_time_histogram.observe(millis as f64);
  }
}
