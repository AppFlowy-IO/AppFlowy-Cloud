use prometheus_client::metrics::{counter::Counter, histogram::Histogram};
use prometheus_client::registry::Registry;
#[derive(Clone)]
pub struct EmbeddingMetrics {
  total_embed_count: Counter,
  failed_embed_count: Counter,
  write_embedding_time_histogram: Histogram,
  gen_embeddings_time_histogram: Histogram,
  fallback_background_tasks: Counter,
}

impl EmbeddingMetrics {
  fn init() -> Self {
    Self {
      total_embed_count: Counter::default(),
      failed_embed_count: Counter::default(),
      write_embedding_time_histogram: Histogram::new([500.0, 1000.0, 5000.0, 8000.0].into_iter()),
      gen_embeddings_time_histogram: Histogram::new([1000.0, 3000.0, 5000.0, 8000.0].into_iter()),
      fallback_background_tasks: Counter::default(),
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
      "write_embedding_time_seconds",
      "Histogram of embedding write times",
      metrics.write_embedding_time_histogram.clone(),
    );

    realtime_registry.register(
      "gen_embeddings_time_histogram",
      "Histogram of embedding generation times",
      metrics.gen_embeddings_time_histogram.clone(),
    );

    realtime_registry.register(
      "fallback_background_tasks",
      "Total count of fallback background tasks",
      metrics.fallback_background_tasks.clone(),
    );

    metrics
  }

  pub fn record_embed_count(&self, count: u64) {
    self.total_embed_count.inc_by(count);
  }

  pub fn record_failed_embed_count(&self, count: u64) {
    self.failed_embed_count.inc_by(count);
  }

  pub fn record_fallback_background_tasks(&self, count: u64) {
    self.fallback_background_tasks.inc_by(count);
  }

  pub fn record_write_embedding_time(&self, millis: u128) {
    self.write_embedding_time_histogram.observe(millis as f64);
  }

  pub fn record_gen_embedding_time(&self, num: u32, millis: u128) {
    tracing::trace!("[Embedding]: index {} collabs cost: {}ms", num, millis);
    self.gen_embeddings_time_histogram.observe(millis as f64);
  }
}
