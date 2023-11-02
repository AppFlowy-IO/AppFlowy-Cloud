use crate::component::auth::jwt::UserUuid;

use crate::api::workspace::{COLLAB_OBJECT_ID_PATH, WORKSPACE_ID_PATH};
use actix_router::{Path, Url};
use actix_service::{forward_ready, Service, Transform};
use actix_web::dev::{ResourceDef, ServiceRequest, ServiceResponse};
use actix_web::http::Method;
use actix_web::Error;
use async_trait::async_trait;
use futures_util::future::LocalBoxFuture;

use std::collections::HashMap;
use std::future::{ready, Ready};
use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use tracing::error;

use app_error::AppError;
use uuid::Uuid;

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub enum AccessResource {
  Workspace,
  Collab,
}

/// The access control service for http request.
/// It is used to check the permission of the request if the request is related to workspace or collab.
/// If the request is not related to workspace or collab, it will be skipped.
///
/// The collab and workspace access control can be separated into different traits. Currently, they are
/// combined into one trait.
#[async_trait]
pub trait HttpAccessControlService: Send + Sync {
  fn resource(&self) -> AccessResource;

  #[allow(unused_variables)]
  async fn check_workspace_permission(
    &self,
    workspace_id: &Uuid,
    user_uuid: &UserUuid,
    method: Method,
  ) -> Result<(), AppError> {
    Ok(())
  }

  #[allow(unused_variables)]
  async fn check_collab_permission(
    &self,
    oid: &str,
    user_uuid: &Uuid,
    method: Method,
    path: Path<Url>,
  ) -> Result<(), AppError> {
    Ok(())
  }
}

#[async_trait]
impl<T> HttpAccessControlService for Arc<T>
where
  T: HttpAccessControlService,
{
  fn resource(&self) -> AccessResource {
    self.as_ref().resource()
  }

  async fn check_workspace_permission(
    &self,
    workspace_id: &Uuid,
    user_uuid: &UserUuid,
    method: Method,
  ) -> Result<(), AppError> {
    self
      .as_ref()
      .check_workspace_permission(workspace_id, user_uuid, method)
      .await
  }

  async fn check_collab_permission(
    &self,
    oid: &str,
    user_uuid: &Uuid,
    method: Method,
    path: Path<Url>,
  ) -> Result<(), AppError> {
    self
      .as_ref()
      .check_collab_permission(oid, user_uuid, method, path)
      .await
  }
}

pub type HttpAccessControlServices =
  Arc<HashMap<AccessResource, Arc<dyn HttpAccessControlService>>>;

/// Implement the access control for the workspace and collab.
/// It will check the permission of the request if the request is related to workspace or collab.
#[derive(Clone, Default)]
pub struct WorkspaceAccessControl {
  access_control_services: HttpAccessControlServices,
}

impl WorkspaceAccessControl {
  pub fn new() -> Self {
    Self::default()
  }

  pub fn with_acs<T: HttpAccessControlService + 'static>(
    mut self,
    access_control_service: T,
  ) -> Self {
    let resource = access_control_service.resource();
    Arc::make_mut(&mut self.access_control_services)
      .insert(resource, Arc::new(access_control_service));
    self
  }
}

impl Deref for WorkspaceAccessControl {
  type Target = HttpAccessControlServices;

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
      access_control_service: self.access_control_services.clone(),
    }))
  }
}

/// Each request will be handled by this middleware. It will check the permission of the request
/// if the request is related to workspace or collab. The [WORKSPACE_ID_PATH] and [COLLAB_OBJECT_ID_PATH]
/// are used to identify the workspace and collab.
///
/// For example, if the request path is `/api/workspace/{workspace_id}/collab/{object_id}`, then the
/// [WorkspaceAccessControlMiddleware] will check the permission of the workspace and collab.
///
///
pub struct WorkspaceAccessControlMiddleware<S> {
  service: S,
  access_control_service: HttpAccessControlServices,
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

        let method = req.method().clone();
        let fut = self.service.call(req);
        let services = self.access_control_service.clone();

        Box::pin(async move {
          // If the workspace_id or collab_object_id is not present, skip the access control
          if workspace_id.is_some() || collab_object_id.is_some() {
            let user_uuid = user_uuid.await?;

            // check workspace permission
            if let Some(workspace_id) = workspace_id {
              if let Some(acs) = services.get(&AccessResource::Workspace) {
                if let Err(err) = acs
                  .check_workspace_permission(&workspace_id, &user_uuid, method.clone())
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
                  .check_collab_permission(&collab_object_id, &user_uuid, method, path)
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
