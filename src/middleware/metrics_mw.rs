use actix_service::{forward_ready, Service, Transform};
use actix_web::dev::{ServiceRequest, ServiceResponse};
use actix_web::web::Data;
use actix_web::Error;
use futures_util::future::LocalBoxFuture;
use std::future::{ready, Ready};
use std::sync::Arc;

use crate::api::metrics::AppFlowyCloudMetrics;

pub struct MetricsMiddleware;

impl<S, B> Transform<S, ServiceRequest> for MetricsMiddleware
where
  S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
  S::Future: 'static,
  B: 'static,
{
  type Response = ServiceResponse<B>;
  type Error = Error;
  type Transform = MetricsMiddlewareService<S>;
  type InitError = ();
  type Future = Ready<Result<Self::Transform, Self::InitError>>;

  fn new_transform(&self, service: S) -> Self::Future {
    ready(Ok(MetricsMiddlewareService { service }))
  }
}

pub struct MetricsMiddlewareService<S> {
  service: S,
}

impl<S, B> Service<ServiceRequest> for MetricsMiddlewareService<S>
where
  S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
  S::Future: 'static,
  B: 'static,
{
  type Response = ServiceResponse<B>;
  type Error = Error;
  type Future = LocalBoxFuture<'static, Result<Self::Response, Self::Error>>;

  forward_ready!(service);

  fn call(&self, req: ServiceRequest) -> Self::Future {
    // Get the metrics from the app_data
    let metrics = req.app_data::<Data<Arc<AppFlowyCloudMetrics>>>().unwrap();

    // Increase the request count for path
    if let Some(pattern) = req.match_pattern() {
      metrics.increase_request_count(pattern);
    }

    // Call the next service
    let res = self.service.call(req);
    Box::pin(res)
  }
}
