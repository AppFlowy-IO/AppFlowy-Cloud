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
  #[instrument(level = "info", skip_all)]
  pub async fn update_member(&self, uid: &i64, oid: &str, access_level: AFAccessLevel) {
    let _ = self
      .access_control
      .update(
        uid,
        &ObjectType::Collab(oid),
        &ActionType::Level(access_level),
      )
      .await;
  }
  pub async fn remove_member(&self, uid: &i64, oid: &str) {
    let _ = self
      .access_control
      .remove(uid, &ObjectType::Collab(oid))
      .await;
  }
}

#[async_trait]
impl CollabAccessControl for CollabAccessControlImpl {
  async fn get_collab_access_level(&self, uid: &i64, oid: &str) -> Result<AFAccessLevel, AppError> {
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

  #[instrument(level = "trace", skip_all)]
  async fn insert_collab_access_level(
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
    self
      .access_control
      .enforce(uid, &ObjectType::Collab(oid), Action::Write)
      .await
  }

  async fn can_receive_collab_update(&self, uid: &i64, oid: &str) -> Result<bool, AppError> {
    self
      .access_control
      .enforce(uid, &ObjectType::Collab(oid), Action::Read)
      .await
  }
}
