use crate::component::auth::jwt::UserUuid;

use actix_service::{forward_ready, Service, Transform};
use actix_web::dev::{ServiceRequest, ServiceResponse};
use actix_web::Error;
use futures_util::future::LocalBoxFuture;
use shared_entity::app_error::AppError;
use sqlx::PgPool;
use std::collections::HashMap;
use std::future::{ready, Ready};
use std::ops::{Deref, DerefMut};
use std::sync::Arc;

pub trait AccessControlService: Send + Sync {
  fn check_permission(&self, user_uuid: UserUuid, pg_pool: &PgPool) -> Result<(), AppError>;
}

impl<T> AccessControlService for Arc<T>
where
  T: AccessControlService,
{
  fn check_permission(&self, user_uuid: UserUuid, pg_pool: &PgPool) -> Result<(), AppError> {
    self.as_ref().check_permission(user_uuid, pg_pool)
  }
}

pub type ResourcePattern = String;
pub type AccessControlServices = Arc<HashMap<ResourcePattern, Arc<dyn AccessControlService>>>;

#[derive(Clone)]
pub struct AccessControl {
  pg_pool: PgPool,
  access_control_services: AccessControlServices,
}

impl AccessControl {
  pub fn new(pg_pool: PgPool) -> Self {
    Self {
      pg_pool,
      access_control_services: Arc::new(Default::default()),
    }
  }

  pub fn extend(&mut self, others: HashMap<String, Arc<dyn AccessControlService>>) {
    Arc::make_mut(&mut self.access_control_services).extend(others)
  }
}

impl Deref for AccessControl {
  type Target = AccessControlServices;

  fn deref(&self) -> &Self::Target {
    &self.access_control_services
  }
}

impl DerefMut for AccessControl {
  fn deref_mut(&mut self) -> &mut Self::Target {
    &mut self.access_control_services
  }
}

impl<S, B> Transform<S, ServiceRequest> for AccessControl
where
  S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
  S::Future: 'static,
  B: 'static,
{
  type Response = ServiceResponse<B>;
  type Error = Error;
  type Transform = AccessControlMiddleware<S>;
  type InitError = ();
  type Future = Ready<Result<Self::Transform, Self::InitError>>;

  fn new_transform(&self, service: S) -> Self::Future {
    ready(Ok(AccessControlMiddleware {
      service,
      pg_pool: self.pg_pool.clone(),
      access_control_service: self.access_control_services.clone(),
    }))
  }
}

pub struct AccessControlMiddleware<S> {
  service: S,
  pg_pool: PgPool,
  access_control_service: AccessControlServices,
}

impl<S, B> Service<ServiceRequest> for AccessControlMiddleware<S>
where
  S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
  S::Future: 'static,
  B: 'static,
{
  type Response = ServiceResponse<B>;
  type Error = Error;
  type Future = LocalBoxFuture<'static, Result<Self::Response, Self::Error>>;

  forward_ready!(service);

  fn call(&self, mut req: ServiceRequest) -> Self::Future {
    let context = req.match_pattern().and_then(|pattern| {
      let acs = self.access_control_service.get(&pattern).cloned()?;
      let user_uuid = req.extract::<UserUuid>();
      Some((acs, user_uuid, self.pg_pool.clone()))
    });

    let fut = self.service.call(req);
    match context {
      None => Box::pin(fut),
      Some((access_control, user_uuid, pg_pool)) => Box::pin(async move {
        if let Ok(user_uuid) = user_uuid.await {
          access_control
            .check_permission(user_uuid, &pg_pool)
            .map_err(Error::from)?;
        }
        fut.await
      }),
    }
  }
}
