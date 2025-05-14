use prometheus_client::{
  encoding::EncodeLabelSet,
  metrics::{counter::Counter, family::Family},
};

#[derive(Default, Clone)]
pub struct AIMetrics {
  total_stream_count: Counter,
  failed_stream_count: Counter,
  stream_image_count: Counter,
  total_completion_count: Counter,
  total_summary_row_count: Counter,
  total_translate_row_count: Counter,
  prompt_usage_count: Family<PromptLabel, Counter>,
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
    realtime_registry.register(
      "total_completion_count",
      "Total count of completions processed",
      metrics.total_completion_count.clone(),
    );
    realtime_registry.register(
      "total_summary_row_count",
      "Total count of summary rows processed",
      metrics.total_summary_row_count.clone(),
    );
    realtime_registry.register(
      "total_translate_row_count",
      "Total count of translation rows processed",
      metrics.total_translate_row_count.clone(),
    );
    realtime_registry.register(
      "prompt_usage_count",
      "Prompt usage count by prompt id",
      metrics.prompt_usage_count.clone(),
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

  pub fn record_total_completion_count(&self, count: u64) {
    self.total_completion_count.inc_by(count);
  }

  pub fn record_total_summary_row_count(&self, count: u64) {
    self.total_summary_row_count.inc_by(count);
  }

  pub fn record_total_translate_row_count(&self, count: u64) {
    self.total_translate_row_count.inc_by(count);
  }

  pub fn record_prompt_usage_count(&self, prompt_id: &str, count: u64) {
    self
      .prompt_usage_count
      .get_or_create(&PromptLabel {
        prompt_id: prompt_id.to_string(),
      })
      .inc_by(count);
  }
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, EncodeLabelSet)]
pub struct PromptLabel {
  pub prompt_id: String,
}
