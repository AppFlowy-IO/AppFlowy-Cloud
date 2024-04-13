use actix_http::header::{HeaderName, HeaderValue};
use std::future::{ready, Ready};
use tracing::{span, Instrument, Level};

use actix_service::{forward_ready, Service, Transform};
use actix_web::dev::{ServiceRequest, ServiceResponse};
use futures_util::future::LocalBoxFuture;

const X_REQUEST_ID: &str = "x-request-id";
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

  fn call(&self, mut req: ServiceRequest) -> Self::Future {
    // Skip generate request id for metrics requests
    if skip_request_id(&req) {
      let fut = self.service.call(req);
      Box::pin(fut)
    } else {
      let request_id = get_request_id(&req).unwrap_or_else(|| {
        let request_id = uuid::Uuid::new_v4().to_string();
        if let Ok(header_value) = HeaderValue::from_str(&request_id) {
          req
            .headers_mut()
            .insert(HeaderName::from_static(X_REQUEST_ID), header_value);
        }
        request_id
      });

      let client_info = get_client_info(&req);
      let span = span!(Level::INFO, "request",
        request_id = %request_id,
        path = %req.match_pattern().unwrap_or_default(),
        method = %req.method(),
        client_version = client_info.client_version,
        device_id = client_info.device_id,
        payload_size = client_info.payload_size
      );

      let fut = self.service.call(req);
      Box::pin(async move {
        let mut res = fut.instrument(span).await?;

        // Insert the request id to the response header
        if let Ok(header_value) = HeaderValue::from_str(&request_id) {
          res
            .headers_mut()
            .insert(HeaderName::from_static(X_REQUEST_ID), header_value);
        }
        Ok(res)
      })
    }
  }
}

pub fn get_request_id(req: &ServiceRequest) -> Option<String> {
  match req.headers().get(HeaderName::from_static(X_REQUEST_ID)) {
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

#[inline]
fn get_client_info(req: &ServiceRequest) -> ClientInfo {
  let payload_size = req
    .headers()
    .get("content-length")
    .and_then(|val| val.to_str().ok())
    .and_then(|val| val.parse::<usize>().ok())
    .unwrap_or_default();

  let client_version = req
    .headers()
    .get("client-version")
    .and_then(|val| val.to_str().ok());

  let device_id = req
    .headers()
    .get("device_id")
    .and_then(|val| val.to_str().ok());

  ClientInfo {
    payload_size,
    client_version,
    device_id,
  }
}

struct ClientInfo<'a> {
  payload_size: usize,
  client_version: Option<&'a str>,
  device_id: Option<&'a str>,
}

#[inline]
fn skip_request_id(req: &ServiceRequest) -> bool {
  if req.path().starts_with("/metrics") {
    return true;
  }

  if req.path().starts_with("/ws") {
    return true;
  }

  false
}
