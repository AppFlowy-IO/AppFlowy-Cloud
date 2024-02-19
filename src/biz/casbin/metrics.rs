use prometheus_client::metrics::gauge::Gauge;
use prometheus_client::metrics::histogram::{exponential_buckets, Histogram};
use prometheus_client::registry::Registry;

#[derive(Clone)]
pub struct AccessControlMetrics {
  load_all_policies: Gauge,
  enforce_duration: Histogram,
}

impl AccessControlMetrics {
  fn init() -> Self {
    let buckets = exponential_buckets(0.005, 2.0, 6);
    let enforce_duration = Histogram::new(buckets.into_iter());
    Self {
      load_all_policies: Gauge::default(),
      enforce_duration,
    }
  }

  pub fn register(registry: &mut Registry) -> Self {
    let metrics = Self::init();
    let realtime_registry = registry.sub_registry_with_prefix("ac");
    realtime_registry.register(
      "load_all_polices",
      "load all polices when server start duration in seconds",
      metrics.load_all_policies.clone(),
    );

    realtime_registry.register(
      "enforce_duration",
      "Duration of enforce calls in milliseconds",
      metrics.enforce_duration.clone(),
    );

    metrics
  }

  pub fn record_load_all_policies_in_secs(&self, millis: u64) {
    self.load_all_policies.set(millis as i64);
  }

  pub fn record_enforce_duration(&self, duration: u64) {
    self.enforce_duration.observe(duration as f64);
  }
}
