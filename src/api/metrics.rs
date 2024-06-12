use actix_web::web;
use actix_web::HttpResponse;
use actix_web::Result;
use actix_web::Scope;
use prometheus_client::encoding::text::encode;
use prometheus_client::encoding::EncodeLabelSet;
use prometheus_client::metrics::counter::Counter;
use prometheus_client::metrics::exemplar::CounterWithExemplar;
use prometheus_client::metrics::family::Family;
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
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
pub struct ResultLabel {
  pub path: String,
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
  pub fn record_request(&self, trace_id: Option<String>, path: String, ms: u64, status_code: u16) {
    self
      .requests_count
      .get_or_create(&PathLabel { path: path.clone() })
      .inc();
    self
      .requests_latency
      .get_or_create(&PathLabel { path: path.clone() })
      .inc_by(ms, trace_id.clone().map(|s| TraceLabel { trace_id: s }));
    self
      .requests_result
      .get_or_create(&ResultLabel { path, status_code })
      .inc_by(1, trace_id.clone().map(|s| TraceLabel { trace_id: s }));
  }
}
