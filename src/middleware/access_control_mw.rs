use crate::api::workspace::{COLLAB_OBJECT_ID_PATH, WORKSPACE_ID_PATH};
use crate::state::AppState;
use access_control::access::enable_access_control;
use actix_router::{Path, Url};
use actix_service::{forward_ready, Service, Transform};
use actix_web::dev::{ResourceDef, ServiceRequest, ServiceResponse};
use actix_web::http::Method;
use actix_web::web::Data;
use actix_web::Error;
use app_error::AppError;
use async_trait::async_trait;
use authentication::jwt::UserUuid;
use dashmap::DashMap;
use futures_util::future::LocalBoxFuture;
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::future::{ready, Ready};
use std::sync::Arc;
use tracing::error;
use uuid::Uuid;

static RESOURCE_DEF_CACHE: Lazy<DashMap<String, ResourceDef>> = Lazy::new(DashMap::new);

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
pub trait MiddlewareAccessControl: Send + Sync {
  fn resource(&self) -> AccessResource;

  #[allow(unused_variables)]
  async fn check_resource_permission(
    &self,
    workspace_id: &str,
    uid: &i64,
    resource_id: &str,
    method: Method,
    path: &Path<Url>,
  ) -> Result<(), AppError>;
}

/// Implement the access control for the workspace and collab.
/// It will check the permission of the request if the request is related to workspace or collab.
#[derive(Clone, Default)]
pub struct MiddlewareAccessControlTransform {
  controllers: Arc<HashMap<AccessResource, Arc<dyn MiddlewareAccessControl>>>,
}

impl MiddlewareAccessControlTransform {
  pub fn new() -> Self {
    Self::default()
  }

  pub fn with_acs<T: MiddlewareAccessControl + 'static>(
    mut self,
    access_control_service: T,
  ) -> Self {
    let resource = access_control_service.resource();
    Arc::make_mut(&mut self.controllers).insert(resource, Arc::new(access_control_service));
    self
  }
}

impl<S, B> Transform<S, ServiceRequest> for MiddlewareAccessControlTransform
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
      controllers: self.controllers.clone(),
    }))
  }
}

/// Each request will be handled by this middleware. It will check the permission of the request
/// if the request is related to workspace or collab. The [WORKSPACE_ID_PATH] and [COLLAB_OBJECT_ID_PATH]
/// are used to identify the workspace and collab.
///
/// For example, if the request path is `/api/workspace/{workspace_id}/collab/{object_id}`, then the
/// [AccessControlMiddleware] will check the permission of the workspace and collab.
///
///
pub struct AccessControlMiddleware<S> {
  service: S,
  controllers: Arc<HashMap<AccessResource, Arc<dyn MiddlewareAccessControl>>>,
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
    // If the access control is not enabled, skip the access control
    if !enable_access_control() {
      let fut = self.service.call(req);
      return Box::pin(fut);
    }

    let path = req.match_pattern().map(|pattern| {
      // Create ResourceDef will cause memory leak, so we use the cache to store the ResourceDef
      let mut path = req.match_info().clone();
      RESOURCE_DEF_CACHE
        .entry(pattern.to_owned())
        .or_insert_with(|| ResourceDef::new(pattern))
        .value()
        .capture_match_info(&mut path);
      path
    });

    match path {
      None => {
        let fut = self.service.call(req);
        Box::pin(fut)
      },
      Some(path) => {
        let user_uuid = req.extract::<UserUuid>();
        let user_cache = req
          .app_data::<Data<AppState>>()
          .map(|state| state.user_cache.clone());

        let uid = async {
          let user_uuid = user_uuid.await.map_err(|err| {
            AppError::Internal(anyhow::anyhow!(
              "Can't find the user uuid from the request: {}",
              err
            ))
          })?;

          user_cache
            .ok_or_else(|| {
              AppError::Internal(anyhow::anyhow!("AppState is not found in the request"))
            })?
            .get_user_uid(&user_uuid)
            .await
        };

        let workspace_id = path
          .get(WORKSPACE_ID_PATH)
          .and_then(|id| Uuid::parse_str(id).ok());
        let object_id = path.get(COLLAB_OBJECT_ID_PATH).map(|id| id.to_string());

        let method = req.method().clone();
        let fut = self.service.call(req);
        let services = self.controllers.clone();

        Box::pin(async move {
          // If the workspace_id or collab_object_id is not present, skip the access control
          if workspace_id.is_none() && object_id.is_none() {
            return fut.await;
          }

          let uid = uid.await?;
          // check workspace permission
          if let Some(workspace_id) = workspace_id {
            let workspace_id = workspace_id.to_string();
            if let Some(workspace_ac) = services.get(&AccessResource::Workspace) {
              if let Err(err) = workspace_ac
                .check_resource_permission(
                  &workspace_id,
                  &uid,
                  &workspace_id,
                  method.clone(),
                  &path,
                )
                .await
              {
                error!("workspace access control: {}", err,);
                return Err(Error::from(err));
              }
            };

            // check collab permission
            if let Some(collab_object_id) = object_id {
              if let Some(collab_ac) = services.get(&AccessResource::Collab) {
                if let Err(err) = collab_ac
                  .check_resource_permission(&workspace_id, &uid, &collab_object_id, method, &path)
                  .await
                {
                  error!(
                    "collab access control: {:?}, with path:{}",
                    err,
                    path.as_str()
                  );
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
