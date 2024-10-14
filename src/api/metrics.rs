use actix_web::web;
use actix_web::HttpResponse;
use actix_web::Result;
use actix_web::Scope;
use prometheus_client::encoding::text::encode;
use prometheus_client::encoding::EncodeLabelSet;
use prometheus_client::metrics::counter::Counter;
use prometheus_client::metrics::exemplar::CounterWithExemplar;
use prometheus_client::metrics::family::Family;
use prometheus_client::metrics::gauge::Gauge;
use prometheus_client::metrics::histogram::exponential_buckets;
use prometheus_client::metrics::histogram::Histogram;
use prometheus_client::registry::Registry;
use std::sync::Arc;
use uuid::Uuid;

pub fn metrics_scope() -> Scope {
  web::scope("/metrics").service(web::resource("").route(web::get().to(metrics_handler)))
}

async fn metrics_handler(reg: web::Data<Arc<Registry>>) -> Result<HttpResponse> {
  let mut body = String::new();
  encode(&mut body, &reg).map_err(|e| {
    tracing::error!("Failed to encode metrics: {:?}", e);
    actix_web::error::ErrorInternalServerError(e)
  })?;
  Ok(
    HttpResponse::Ok()
      .content_type("application/openmetrics-text; version=1.0.0; charset=utf-8")
      .body(body),
  )
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
pub struct PathLabel {
  pub path: String,
  pub method: String,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
pub struct ResultLabel {
  pub path: String,
  pub method: String,
  pub status_code: u16,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
pub struct WorkspaceLabel {
  pub workspace: String,
}

// Metrics contains list of metrics that are collected by the application.
// Metric types: https://prometheus.io/docs/concepts/metric_types
// Application handlers should call the corresponding methods to update the metrics.
#[derive(Clone)]
pub struct RequestMetrics {
  requests_count: Family<PathLabel, Counter>,
  requests_latency: Family<PathLabel, CounterWithExemplar<TraceLabel>>,
  requests_result: Family<ResultLabel, CounterWithExemplar<TraceLabel>>,
  openai_token_usage: Family<WorkspaceLabel, Counter>,
}

#[derive(Clone, Hash, PartialEq, Eq, EncodeLabelSet, Debug, Default)]
pub struct TraceLabel {
  pub trace_id: String,
}

impl RequestMetrics {
  fn init() -> Self {
    Self {
      requests_count: Family::default(),
      requests_latency: Family::default(),
      requests_result: Family::default(),
      openai_token_usage: Family::default(),
    }
  }

  pub fn register(registry: &mut Registry) -> Self {
    let af_metrics = Self::init();

    let af_registry = registry.sub_registry_with_prefix("appflowy_cloud");
    af_registry.register(
      "requests_count",
      "number of requests",
      af_metrics.requests_count.clone(),
    );
    af_registry.register(
      "requests_latency",
      "request response time",
      af_metrics.requests_latency.clone(),
    );
    af_registry.register(
      "requests_result",
      "status code of response",
      af_metrics.requests_result.clone(),
    );
    af_registry.register(
      "search_tokens_used",
      "OpenAI API tokens used for search requests",
      af_metrics.openai_token_usage.clone(),
    );
    af_metrics
  }

  pub fn record_search_tokens_used(&self, workspace_id: &Uuid, tokens: u32) {
    self
      .openai_token_usage
      .get_or_create(&WorkspaceLabel {
        workspace: workspace_id.to_string(),
      })
      .inc_by(tokens as u64);
  }

  // app services/middleware should call this method to increase the request count for the path
  pub fn record_request(
    &self,
    trace_id: Option<String>,
    path: String,
    method: String,
    ms: u64,
    status_code: u16,
  ) {
    self
      .requests_count
      .get_or_create(&PathLabel {
        path: path.clone(),
        method: method.clone(),
      })
      .inc();
    self
      .requests_latency
      .get_or_create(&PathLabel {
        path: path.clone(),
        method: method.clone(),
      })
      .inc_by(ms, trace_id.clone().map(|s| TraceLabel { trace_id: s }));
    self
      .requests_result
      .get_or_create(&ResultLabel {
        path,
        method,
        status_code,
      })
      .inc_by(1, trace_id.clone().map(|s| TraceLabel { trace_id: s }));
  }
}

#[derive(Clone)]
pub struct PublishedCollabMetrics {
  success_write_published_collab_count: Gauge,
  fallback_write_published_collab_count: Gauge,
  failure_write_published_collab_count: Gauge,
  success_read_published_collab_count: Gauge,
  fallback_read_published_collab_count: Gauge,
  failure_read_published_collab_count: Gauge,
}

impl PublishedCollabMetrics {
  fn init() -> Self {
    Self {
      success_write_published_collab_count: Default::default(),
      fallback_write_published_collab_count: Default::default(),
      failure_write_published_collab_count: Default::default(),
      success_read_published_collab_count: Default::default(),
      fallback_read_published_collab_count: Default::default(),
      failure_read_published_collab_count: Default::default(),
    }
  }

  pub fn register(registry: &mut Registry) -> Self {
    let metrics = Self::init();
    let published_collab_registry = registry.sub_registry_with_prefix("published_collab");
    published_collab_registry.register(
      "write_success_count",
      "successfully published collab",
      metrics.success_write_published_collab_count.clone(),
    );
    published_collab_registry.register(
      "write_fallback_count",
      "successfully published collab to fallback store",
      metrics.fallback_write_published_collab_count.clone(),
    );
    published_collab_registry.register(
      "read_success_count",
      "successfully read published collab",
      metrics.success_read_published_collab_count.clone(),
    );
    published_collab_registry.register(
      "write_failure_count",
      "failed to publish collab",
      metrics.failure_write_published_collab_count.clone(),
    );
    published_collab_registry.register(
      "read_failure_count",
      "failed to read published collab",
      metrics.failure_read_published_collab_count.clone(),
    );
    published_collab_registry.register(
      "read_fallback_count",
      "failed to read published collab from primary store",
      metrics.fallback_read_published_collab_count.clone(),
    );

    metrics
  }

  pub fn incr_success_write_count(&self, count: i64) {
    self.success_write_published_collab_count.inc_by(count);
  }

  pub fn incr_fallback_write_count(&self, count: i64) {
    self.fallback_write_published_collab_count.inc_by(count);
  }

  pub fn incr_failure_write_count(&self, count: i64) {
    self.failure_write_published_collab_count.inc_by(count);
  }

  pub fn incr_success_read_count(&self, count: i64) {
    self.success_read_published_collab_count.inc_by(count);
  }

  pub fn incr_fallback_read_count(&self, count: i64) {
    self.fallback_read_published_collab_count.inc_by(count);
  }

  pub fn incr_failure_read_count(&self, count: i64) {
    self.failure_read_published_collab_count.inc_by(count);
  }
}

pub struct AppFlowyWebMetrics {
  pub update_size_bytes: Histogram,
  pub decoding_failure_count: Gauge,
  pub apply_update_failure_count: Gauge,
}

impl AppFlowyWebMetrics {
  pub fn init() -> Self {
    let update_size_buckets = exponential_buckets(1024.0, 2.0, 10);

    Self {
      update_size_bytes: Histogram::new(update_size_buckets),
      decoding_failure_count: Default::default(),
      apply_update_failure_count: Default::default(),
    }
  }

  pub fn register(registry: &mut Registry) -> Self {
    let metrics = Self::init();
    let web_update_registry = registry.sub_registry_with_prefix("appflowy_web");
    web_update_registry.register(
      "update_size_bytes",
      "Size of the update in bytes",
      metrics.update_size_bytes.clone(),
    );
    web_update_registry.register(
      "decoding_failure_count",
      "Number of updates that failed to decode",
      metrics.decoding_failure_count.clone(),
    );
    web_update_registry.register(
      "apply_update_failure_count",
      "Number of updates that failed to apply",
      metrics.apply_update_failure_count.clone(),
    );
    metrics
  }

  pub fn record_update_size_bytes(&self, size: usize) {
    self.update_size_bytes.observe(size as f64);
  }

  pub fn incr_decoding_failure_count(&self, count: i64) {
    self.decoding_failure_count.inc_by(count);
  }

  pub fn incr_apply_update_failure_count(&self, count: i64) {
    self.apply_update_failure_count.inc_by(count);
  }
}
