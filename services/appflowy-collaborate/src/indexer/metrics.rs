use prometheus_client::metrics::{counter::Counter, histogram::Histogram};
use prometheus_client::registry::Registry;
#[derive(Clone)]
pub struct EmbeddingMetrics {
  total_embed_count: Counter,
  failed_embed_count: Counter,
  processing_time_histogram: Histogram,
  write_embedding_time_histogram: Histogram,
  handle_batch_unindexed_collab_time_histogram: Histogram,
}

impl EmbeddingMetrics {
  fn init() -> Self {
    Self {
      total_embed_count: Counter::default(),
      failed_embed_count: Counter::default(),
      processing_time_histogram: Histogram::new([500.0, 1000.0, 5000.0, 8000.0].into_iter()),
      write_embedding_time_histogram: Histogram::new([500.0, 1000.0, 5000.0, 8000.0].into_iter()),
      handle_batch_unindexed_collab_time_histogram: Histogram::new(
        [1000.0, 3000.0, 5000.0, 8000.0].into_iter(),
      ),
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
    realtime_registry.register(
      "write_embedding_time_seconds",
      "Histogram of embedding write times",
      metrics.write_embedding_time_histogram.clone(),
    );

    realtime_registry.register(
      "handle_batch_unindexed_collab_time_histogram",
      "Histogram of unindexed collab pg query times",
      metrics.handle_batch_unindexed_collab_time_histogram.clone(),
    );

    metrics
  }

  pub fn record_embed_count(&self, count: u64) {
    self.total_embed_count.inc_by(count);
  }

  pub fn record_failed_embed_count(&self, count: u64) {
    self.failed_embed_count.inc_by(count);
  }

  pub fn record_generate_embedding_time(&self, millis: u128) {
    tracing::trace!("[Embedding]: generate embeddings cost: {}ms", millis);
    self.processing_time_histogram.observe(millis as f64);
  }

  pub fn record_write_embedding_time(&self, millis: u128) {
    tracing::trace!("[Embedding]: write embedding time cost: {}ms", millis);
    self.write_embedding_time_histogram.observe(millis as f64);
  }

  pub fn record_handle_batch_unindexed_collab_time(&self, millis: u128) {
    tracing::trace!(
      "[Embedding]: handle batch unindexed collab cost: {}ms",
      millis
    );
    self
      .handle_batch_unindexed_collab_time_histogram
      .observe(millis as f64);
  }
}
