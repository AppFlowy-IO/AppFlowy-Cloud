use actix_http::Payload;
use actix_service::{forward_ready, Service, Transform};
use actix_web::{dev::ServiceRequest, dev::ServiceResponse, Error};
use bytes::Bytes;
use bytes::BytesMut;
use futures::future::Ready;
use futures_util::future::{ready, LocalBoxFuture};
use futures_util::{stream, StreamExt};

pub struct DecryptPayloadMiddleware;

impl<S, B> Transform<S, ServiceRequest> for DecryptPayloadMiddleware
where
  S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
  S::Future: 'static,
  B: 'static,
{
  type Response = ServiceResponse<B>;
  type Error = Error;
  type Transform = DecryptPayloadMiddlewareService<S>;
  type InitError = ();
  type Future = Ready<Result<Self::Transform, Self::InitError>>;

  fn new_transform(&self, service: S) -> Self::Future {
    ready(Ok(DecryptPayloadMiddlewareService { service }))
  }
}

pub struct DecryptPayloadMiddlewareService<S> {
  service: S,
}

impl<S, B> Service<ServiceRequest> for DecryptPayloadMiddlewareService<S>
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
    let (http_req, mut payload) = req.into_parts();
    let payload_stream = stream::once(async move {
      let mut body = BytesMut::new();
      while let Some(chunk) = payload.next().await {
        body.extend_from_slice(&chunk?);
      }

      Ok(Bytes::from(body))
    });

    let payload = Box::pin(payload_stream);
    let new_req = ServiceRequest::from_parts(http_req, Payload::Stream { payload });
    let fut = self.service.call(new_req);
    Box::pin(fut)
  }
}
