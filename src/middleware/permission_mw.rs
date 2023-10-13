use crate::component::auth::jwt::UserUuid;

use crate::api::workspace::WORKSPACE_ID_PATH;
use actix_service::{forward_ready, Service, Transform};
use actix_web::dev::{ResourceDef, ServiceRequest, ServiceResponse};

use actix_web::Error;
use async_trait::async_trait;
use futures_util::future::LocalBoxFuture;
use shared_entity::app_error::AppError;
use sqlx::PgPool;
use std::collections::HashMap;
use std::future::{ready, Ready};
use std::ops::{Deref, DerefMut};
use std::sync::Arc;

use uuid::Uuid;

#[async_trait]
pub trait WorkspaceAccessControlService: Send + Sync {
  async fn check_permission(
    &self,
    workspace_id: Uuid,
    user_uuid: UserUuid,
    pg_pool: &PgPool,
  ) -> Result<(), AppError>;
}

#[async_trait]
impl<T> WorkspaceAccessControlService for Arc<T>
where
  T: WorkspaceAccessControlService,
{
  async fn check_permission(
    &self,
    workspace_id: Uuid,
    user_uuid: UserUuid,
    pg_pool: &PgPool,
  ) -> Result<(), AppError> {
    self
      .as_ref()
      .check_permission(workspace_id, user_uuid, pg_pool)
      .await
  }
}

pub type ResourcePattern = String;
pub type AccessControlServices =
  Arc<HashMap<ResourcePattern, Arc<dyn WorkspaceAccessControlService>>>;

#[derive(Clone)]
pub struct WorkspaceAccessControl {
  pg_pool: PgPool,
  access_control_services: AccessControlServices,
}

impl WorkspaceAccessControl {
  pub fn new(pg_pool: PgPool) -> Self {
    Self {
      pg_pool,
      access_control_services: Arc::new(Default::default()),
    }
  }

  pub fn extend(&mut self, others: HashMap<String, Arc<dyn WorkspaceAccessControlService>>) {
    Arc::make_mut(&mut self.access_control_services).extend(others)
  }
}

impl Deref for WorkspaceAccessControl {
  type Target = AccessControlServices;

  fn deref(&self) -> &Self::Target {
    &self.access_control_services
  }
}

impl DerefMut for WorkspaceAccessControl {
  fn deref_mut(&mut self) -> &mut Self::Target {
    &mut self.access_control_services
  }
}

impl<S, B> Transform<S, ServiceRequest> for WorkspaceAccessControl
where
  S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
  S::Future: 'static,
  B: 'static,
{
  type Response = ServiceResponse<B>;
  type Error = Error;
  type Transform = WorkspaceAccessControlMiddleware<S>;
  type InitError = ();
  type Future = Ready<Result<Self::Transform, Self::InitError>>;

  fn new_transform(&self, service: S) -> Self::Future {
    ready(Ok(WorkspaceAccessControlMiddleware {
      service,
      pg_pool: self.pg_pool.clone(),
      access_control_service: self.access_control_services.clone(),
    }))
  }
}

pub struct WorkspaceAccessControlMiddleware<S> {
  service: S,
  pg_pool: PgPool,
  access_control_service: AccessControlServices,
}

impl<S, B> Service<ServiceRequest> for WorkspaceAccessControlMiddleware<S>
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
    let match_pattern = req.match_pattern();
    let access_control = match_pattern
      .as_ref()
      .and_then(|pattern| self.access_control_service.get(pattern).cloned());

    match access_control {
      None => {
        let fut = self.service.call(req);
        Box::pin(fut)
      },
      Some(access_control) => {
        let user_uuid = req.extract::<UserUuid>();
        let workspace_id = match_pattern.and_then(|pattern| {
          let resource_ref = ResourceDef::new(pattern);
          let mut path = req.match_info().clone();
          resource_ref.capture_match_info(&mut path);
          path.get(WORKSPACE_ID_PATH).map(Uuid::parse_str)
        });

        let fut = self.service.call(req);
        let pg_pool = self.pg_pool.clone();

        Box::pin(async move {
          if let Some(Ok(workspace_id)) = workspace_id {
            if let Ok(user_uuid) = user_uuid.await {
              access_control
                .check_permission(workspace_id, user_uuid, &pg_pool)
                .await
                .map_err(Error::from)?;
            }
          }
          fut.await
        })
      },
    }
  }
}
