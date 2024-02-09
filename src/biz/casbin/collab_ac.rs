use crate::biz::casbin::access_control::AccessControl;
use crate::biz::casbin::access_control::{
  ActionType, ObjectType, POLICY_FIELD_INDEX_ACTION, POLICY_FIELD_INDEX_OBJECT,
  POLICY_FIELD_INDEX_USER,
};
use actix_http::Method;
use app_error::AppError;
use async_trait::async_trait;
use casbin::MgmtApi;
use database_entity::dto::AFAccessLevel;
use realtime::collaborate::CollabAccessControl;
use std::str::FromStr;
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
    let collab_id = ObjectType::Collab(oid).to_string();
    let policies = self
      .access_control
      .enforcer
      .read()
      .await
      .get_filtered_policy(POLICY_FIELD_INDEX_OBJECT, vec![collab_id]);

    // There should only be one entry per user per object, which is enforced in [AccessControl], so just take one using next.
    let access_level = policies
      .into_iter()
      .find(|p| p[POLICY_FIELD_INDEX_USER] == uid.to_string())
      .map(|p| p[POLICY_FIELD_INDEX_ACTION].clone())
      .and_then(|s| i32::from_str(s.as_str()).ok())
      .map(AFAccessLevel::from);

    access_level.ok_or(AppError::RecordNotFound(format!(
      "user:{} is not a member of collab:{}",
      uid, oid
    )))
  }

  #[instrument(level = "trace", skip_all)]
  async fn cache_collab_access_level(
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
    _uid: &i64,
    _oid: &str,
    _method: &Method,
  ) -> Result<bool, AppError> {
    Ok(true)
    // let action = if Method::POST == method || Method::PUT == method || Method::DELETE == method {
    //   Action::Write
    // } else {
    //   Action::Read
    // };
    //
    // // If collab does not exist, allow access.
    // // Workspace access control will still check it.
    // let collab_exists = self
    //   .casbin_access_control
    //   .enforcer
    //   .read()
    //   .await
    //   .get_all_objects()
    //   .contains(&ObjectType::Collab(oid).to_string());
    //
    // if !collab_exists {
    //   return Ok(true);
    // }
    //
    // self
    //   .casbin_access_control
    //   .enforcer
    //   .read()
    //   .await
    //   .enforce((
    //     uid.to_string(),
    //     ObjectType::Collab(oid).to_string(),
    //     action.to_string(),
    //   ))
    //   .map_err(|e| AppError::Internal(anyhow!("casbin error enforce: {e:?}")))
  }

  async fn can_send_collab_update(&self, _uid: &i64, _oid: &str) -> Result<bool, AppError> {
    Ok(true)
    // self
    //   .casbin_access_control
    //   .enforcer
    //   .read()
    //   .await
    //   .enforce((
    //     uid.to_string(),
    //     ObjectType::Collab(oid).to_string(),
    //     Action::Write.to_string(),
    //   ))
    //   .map_err(|e| AppError::Internal(anyhow!("casbin error enforce: {e:?}")))
  }

  async fn can_receive_collab_update(&self, _uid: &i64, _oid: &str) -> Result<bool, AppError> {
    Ok(true)
    // self
    //   .casbin_access_control
    //   .enforcer
    //   .read()
    //   .await
    //   .enforce((
    //     uid.to_string(),
    //     ObjectType::Collab(oid).to_string(),
    //     Action::Read.to_string(),
    //   ))
    //   .map_err(|e| AppError::Internal(anyhow!("casbin error enforce: {e:?}")))
  }
}
