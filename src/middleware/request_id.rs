use actix_http::header::HeaderName;
use std::future::{ready, Ready};
use tracing::{debug, Instrument, Level};

use actix_service::{forward_ready, Service, Transform};
use actix_web::dev::{ServiceRequest, ServiceResponse};
use futures_util::future::LocalBoxFuture;

pub struct RequestIdMiddleware;

impl<S, B> Transform<S, ServiceRequest> for RequestIdMiddleware
where
  S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = actix_web::Error>,
  S::Future: 'static,
  B: 'static,
{
  type Response = ServiceResponse<B>;
  type Error = actix_web::Error;
  type Transform = RequestIdMiddlewareService<S>;
  type InitError = ();
  type Future = Ready<Result<Self::Transform, Self::InitError>>;

  fn new_transform(&self, service: S) -> Self::Future {
    ready(Ok(RequestIdMiddlewareService { service }))
  }
}

pub struct RequestIdMiddlewareService<S> {
  service: S,
}

impl<S, B> Service<ServiceRequest> for RequestIdMiddlewareService<S>
where
  S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = actix_web::Error>,
  S::Future: 'static,
  B: 'static,
{
  type Response = ServiceResponse<B>;
  type Error = actix_web::Error;
  type Future = LocalBoxFuture<'static, Result<Self::Response, Self::Error>>;

  forward_ready!(service);

  fn call(&self, req: ServiceRequest) -> Self::Future {
    let request_id = get_request_id(&req).unwrap_or(uuid::Uuid::new_v4().to_string());
    debug!("generated request id for: {}", req.path());

    // Call the next service
    let span = tracing::span!(Level::INFO, "request_id", request_id = %request_id);
    let res = self.service.call(req);
    Box::pin(res.instrument(span))
  }
}

pub fn get_request_id(req: &ServiceRequest) -> Option<String> {
  match req.headers().get(HeaderName::from_static("x-request-id")) {
    Some(h) => match h.to_str() {
      Ok(s) => Some(s.to_owned()),
      Err(e) => {
        tracing::error!("Failed to get request id from header: {}", e);
        None
      },
    },
    None => None,
  }
}
