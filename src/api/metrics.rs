use std::sync::Arc;

use actix_web::web;
use actix_web::HttpResponse;
use actix_web::Result;
use actix_web::Scope;
use prometheus_client::encoding::text::encode;
use prometheus_client::encoding::EncodeLabelSet;
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

pub fn metrics_registry() -> (AppFlowyCloudMetrics, Registry) {
  let metric = AppFlowyCloudMetrics::init();
  let mut registry = Registry::default();
  AppFlowyCloudMetrics::register(metric.clone(), &mut registry);
  (metric, registry)
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
pub struct PathLabels {
  pub path: String,
}

// Metrics contains list of metrics that are collected by the application.
// Metric types: https://prometheus.io/docs/concepts/metric_types
// Application handlers should call the corresponding methods to update the metrics.
#[derive(Clone)]
pub struct AppFlowyCloudMetrics {
  requests: Family<PathLabels, Counter>,
}

impl AppFlowyCloudMetrics {
  fn init() -> Self {
    Self {
      requests: Family::default(),
    }
  }

  fn register(self, registry: &mut Registry) {
    registry.register("requests", "count", self.requests.clone());
  }

  // app services/middleware should call this method to increase the request count for the path
  pub fn increase_request_count(&self, s: String) {
    self.requests.get_or_create(&PathLabels { path: s }).inc();
  }
}
