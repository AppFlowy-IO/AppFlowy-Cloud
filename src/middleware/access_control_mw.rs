use crate::component::auth::jwt::UserUuid;

use crate::api::workspace::{COLLAB_OBJECT_ID_PATH, WORKSPACE_ID_PATH};
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
use tracing::error;

use uuid::Uuid;

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub enum AccessResource {
  Workspace,
  Collab,
}

#[async_trait]
pub trait AccessControlService: Send + Sync {
  fn resource(&self) -> AccessResource;

  async fn check_workspace_permission(
    &self,
    _workspace_id: &Uuid,
    _user_uuid: &UserUuid,
    _pg_pool: &PgPool,
  ) -> Result<(), AppError> {
    Ok(())
  }

  async fn check_collab_permission(
    &self,
    _workspace_id: &Uuid,
    _oid: &str,
    _user_uuid: &UserUuid,
    _pg_pool: &PgPool,
  ) -> Result<(), AppError> {
    Ok(())
  }
}

#[async_trait]
impl<T> AccessControlService for Arc<T>
where
  T: AccessControlService,
{
  fn resource(&self) -> AccessResource {
    self.as_ref().resource()
  }

  async fn check_workspace_permission(
    &self,
    workspace_id: &Uuid,
    user_uuid: &UserUuid,
    pg_pool: &PgPool,
  ) -> Result<(), AppError> {
    self
      .as_ref()
      .check_workspace_permission(workspace_id, user_uuid, pg_pool)
      .await
  }

  async fn check_collab_permission(
    &self,
    _workspace_id: &Uuid,
    _oid: &str,
    _user_uuid: &UserUuid,
    _pg_pool: &PgPool,
  ) -> Result<(), AppError> {
    self
      .as_ref()
      .check_collab_permission(_workspace_id, _oid, _user_uuid, _pg_pool)
      .await
  }
}

pub type AccessControlServices = Arc<HashMap<AccessResource, Arc<dyn AccessControlService>>>;

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

  pub fn with_acs<T: AccessControlService + 'static>(mut self, access_control_service: T) -> Self {
    let resource = access_control_service.resource();
    Arc::make_mut(&mut self.access_control_services)
      .insert(resource, Arc::new(access_control_service));
    self
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
    match req.match_pattern().map(|pattern| {
      let resource_ref = ResourceDef::new(pattern);
      let mut path = req.match_info().clone();
      resource_ref.capture_match_info(&mut path);
      path
    }) {
      None => {
        let fut = self.service.call(req);
        Box::pin(fut)
      },
      Some(path) => {
        let user_uuid = req.extract::<UserUuid>();

        let workspace_id = path
          .get(WORKSPACE_ID_PATH)
          .and_then(|id| Uuid::parse_str(id).ok());
        let collab_object_id = path.get(COLLAB_OBJECT_ID_PATH).map(|id| id.to_string());

        let fut = self.service.call(req);
        let pg_pool = self.pg_pool.clone();
        let services = self.access_control_service.clone();

        Box::pin(async move {
          // If the workspace_id or collab_object_id is not present, skip the access control
          if workspace_id.is_some() || collab_object_id.is_some() {
            let user_uuid = user_uuid.await?;

            // check workspace permission
            if let Some(workspace_id) = workspace_id {
              if let Some(acs) = services.get(&AccessResource::Workspace) {
                if let Err(err) = acs
                  .check_workspace_permission(&workspace_id, &user_uuid, &pg_pool)
                  .await
                {
                  error!("workspace access control: {:?}", err);
                  return Err(Error::from(err));
                }
              };
            }

            // check collab permission
            if let Some(collab_object_id) = collab_object_id {
              if let Some(acs) = services.get(&AccessResource::Collab) {
                if let Err(err) = acs
                  .check_collab_permission(
                    &workspace_id.unwrap(),
                    &collab_object_id,
                    &user_uuid,
                    &pg_pool,
                  )
                  .await
                {
                  error!("collab access control: {:?}", err);
                  return Err(Error::from(err));
                }
              };
            }
          }

          // call next service
          fut.await
        })
      },
    }
  }
}
