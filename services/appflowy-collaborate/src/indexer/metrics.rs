use prometheus_client::metrics::gauge::Gauge;
use prometheus_client::registry::Registry;

#[derive(Clone)]
pub struct EmbeddingMetrics {
  total_embed_count: Gauge,
}

impl EmbeddingMetrics {
  fn init() -> Self {
    Self {
      total_embed_count: Gauge::default(),
    }
  }

  pub fn register(registry: &mut Registry) -> Self {
    let metrics = Self::init();
    let realtime_registry = registry.sub_registry_with_prefix("embedding");

    realtime_registry.register(
      "total_embed_count",
      "total embed count",
      metrics.total_embed_count.clone(),
    );

    metrics
  }

  pub fn record_total_embed_count(&self, total: i64) {
    self.total_embed_count.set(total);
  }
}
