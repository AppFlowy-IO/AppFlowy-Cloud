use prometheus_client::metrics::counter::Counter;
use prometheus_client::registry::Registry;

#[derive(Default)]
pub struct CollabStreamMetrics {
  /// Incremented each time a new collab stream read task is set (including recurring tasks).
  pub reads_enqueued: Counter,
  /// Incremented each time an existing task is consumed (including recurring tasks).
  pub reads_dequeued: Counter,
}

impl CollabStreamMetrics {
  pub fn register(registry: &mut Registry) -> Self {
    let metrics = Self::default();
    let realtime_registry = registry.sub_registry_with_prefix("collab_stream");
    realtime_registry.register(
      "reads_enqueued",
      "Incremented each time a new collab stream read task is set (including recurring tasks).",
      metrics.reads_enqueued.clone(),
    );
    realtime_registry.register(
      "reads_dequeued",
      "Incremented each time an existing task is consumed (including recurring tasks).",
      metrics.reads_dequeued.clone(),
    );
    metrics
  }
}
