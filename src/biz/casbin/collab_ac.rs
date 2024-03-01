use crate::biz::casbin::access_control::{AccessControl, Action};
use crate::biz::casbin::access_control::{ActionType, ObjectType};
use actix_http::Method;
use app_error::AppError;
use async_trait::async_trait;

use database_entity::dto::AFAccessLevel;
use realtime::collaborate::CollabAccessControl;

use tracing::instrument;

#[derive(Clone)]
pub struct CollabAccessControlImpl {
  access_control: AccessControl,
}

impl CollabAccessControlImpl {
  pub fn new(access_control: AccessControl) -> Self {
    Self { access_control }
  }
}

#[async_trait]
impl CollabAccessControl for CollabAccessControlImpl {
  async fn get_access_level(&self, uid: &i64, oid: &str) -> Result<AFAccessLevel, AppError> {
    self
      .access_control
      .get_access_level(uid, oid)
      .await
      .ok_or_else(|| {
        AppError::RecordNotFound(format!(
          "can't find the access level for user:{} of {} in cache",
          uid, oid
        ))
      })
  }

  #[instrument(level = "info", skip_all)]
  async fn insert_access_level(
    &self,
    uid: &i64,
    oid: &str,
    level: AFAccessLevel,
  ) -> Result<(), AppError> {
    self
      .access_control
      .update(uid, &ObjectType::Collab(oid), &ActionType::Level(level))
      .await?;

    Ok(())
  }

  #[instrument(level = "info", skip_all)]
  async fn remove_access_level(&self, uid: &i64, oid: &str) -> Result<(), AppError> {
    self
      .access_control
      .remove(uid, &ObjectType::Collab(oid))
      .await?;
    Ok(())
  }

  async fn can_access_http_method(
    &self,
    uid: &i64,
    oid: &str,
    method: &Method,
  ) -> Result<bool, AppError> {
    let action = Action::from(method);
    self
      .access_control
      .enforce(uid, &ObjectType::Collab(oid), action)
      .await
  }

  async fn can_send_collab_update(&self, uid: &i64, oid: &str) -> Result<bool, AppError> {
    if cfg!(feature = "disable_access_control") {
      Ok(true)
    } else {
      self
        .access_control
        .enforce(uid, &ObjectType::Collab(oid), Action::Write)
        .await
    }
  }

  async fn can_receive_collab_update(&self, uid: &i64, oid: &str) -> Result<bool, AppError> {
    if cfg!(feature = "disable_access_control") {
      Ok(true)
    } else {
      self
        .access_control
        .enforce(uid, &ObjectType::Collab(oid), Action::Read)
        .await
    }
  }
}
