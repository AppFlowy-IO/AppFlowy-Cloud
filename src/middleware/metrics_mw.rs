use actix_service::{forward_ready, Service, Transform};
use actix_web::dev::{ServiceRequest, ServiceResponse};
use actix_web::web::Data;
use actix_web::Error;
use futures_util::future::LocalBoxFuture;
use std::future::{ready, Ready};
use std::sync::Arc;

use super::request_id::get_request_id;
use crate::api::metrics::RequestMetrics;

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
    let metrics = match req.app_data::<Data<Arc<RequestMetrics>>>() {
      Some(m) => m.clone(),
      None => {
        tracing::error!("Failed to get metrics from app_data");
        return Box::pin(self.service.call(req));
      },
    };

    let request_id = get_request_id(&req);
    let endpoint = req.match_pattern();

    // Call the next service
    let res = self.service.call(req);
    Box::pin(async move {
      let start = std::time::Instant::now();
      let res = res.await?;
      let end = std::time::Instant::now();
      let duration = end.duration_since(start);
      let status = res.status();
      if let Some(endpoint) = endpoint {
        metrics.record_request(
          request_id,
          endpoint,
          duration.as_millis() as u64,
          status.into(),
        );
      }
      Ok(res)
    })
  }
}
