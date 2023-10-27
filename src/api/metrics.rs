use std::sync::Arc;

use actix_web::web;
use actix_web::HttpResponse;
use actix_web::Result;
use actix_web::Scope;
use prometheus_client::encoding::text::encode;
use prometheus_client::encoding::EncodeLabelSet;
use prometheus_client::encoding::EncodeLabelValue;
use prometheus_client::metrics::counter::Counter;
use prometheus_client::metrics::family::Family;
use prometheus_client::registry::Registry;

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

pub fn metrics_registry() -> (Metrics, Registry) {
  let request_counters = Family::<MethodLabels, Counter>::default();

  let mut registry = Registry::default();
  registry.register("requests", "count", request_counters.clone());
  (
    Metrics {
      requests: request_counters,
    },
    registry,
  )
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelValue)]
pub enum Method {
  GET,
  POST,
  PUT,
  DELETE,
  PATCH,
  HEAD,
  OPTIONS,
  CONNECT,
  TRACE,
  OTHER,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
pub struct MethodLabels {
  pub method: Method,
}

// Metrics contains list of metrics that are collected by the application.
// Metric types: https://prometheus.io/docs/concepts/metric_types
// Application handlers should call the corresponding methods to update the metrics.
#[derive(Clone)]
pub struct Metrics {
  requests: Family<MethodLabels, Counter>,
}

impl Metrics {
  pub fn inc_requests(&self, method: Method) {
    self.requests.get_or_create(&MethodLabels { method }).inc();
  }
}
