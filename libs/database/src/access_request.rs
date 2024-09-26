use crate::pg_row::{
  AFAccessRequestStatusColumn, AFAccessRequestWithViewIdColumn, AFAccessRequesterColumn,
  AFWorkspaceRow,
};
use app_error::AppError;
use database_entity::dto::AccessRequestWithViewId;
use sqlx::{Executor, Postgres};
use uuid::Uuid;

pub async fn insert_new_access_request<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  workspace_id: Uuid,
  view_id: Uuid,
  uid: i64,
) -> Result<Uuid, AppError> {
  let request_id_result = sqlx::query_scalar!(
    r#"
      INSERT INTO af_access_request (
        workspace_id,
        view_id,
        uid,
        status
      )
      VALUES ($1, $2, $3, $4)
      RETURNING request_id
    "#,
    workspace_id,
    view_id,
    uid,
    AFAccessRequestStatusColumn::Pending as _,
  )
  .fetch_one(executor)
  .await;
  match request_id_result {
    Err(e)
      if e
        .as_database_error()
        .map_or(false, |e| e.constraint().is_some()) =>
    {
      Err(AppError::AccessRequestAlreadyExists {
        workspace_id,
        view_id,
      })
    },
    Err(e) => Err(e.into()),
    Ok(request_id) => Ok(request_id),
  }
}

pub async fn select_access_request_by_request_id<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  request_id: Uuid,
) -> Result<AccessRequestWithViewId, AppError> {
  let access_request = sqlx::query_as!(
    AFAccessRequestWithViewIdColumn,
    r#"
      SELECT
      request_id,
      view_id,
      (
        workspace_id,
        af_workspace.database_storage_id,
        af_workspace.owner_uid,
        owner_profile.name,
        af_workspace.created_at,
        af_workspace.workspace_type,
        af_workspace.deleted_at,
        af_workspace.workspace_name,
        af_workspace.icon
      ) AS "workspace!: AFWorkspaceRow",
      (
        af_user.uuid,
        af_user.email,
        af_user.name,
        af_user.metadata ->> 'avatar'
      ) AS "requester!: AFAccessRequesterColumn",
      status AS "status: AFAccessRequestStatusColumn",
      af_access_request.created_at AS created_at
      FROM af_access_request
      JOIN af_user USING (uid)
      JOIN af_workspace USING (workspace_id)
      JOIN af_user AS owner_profile ON af_workspace.owner_uid = owner_profile.uid
      WHERE request_id = $1
    "#,
    request_id,
  )
  .fetch_one(executor)
  .await?;

  let access_request: AccessRequestWithViewId = access_request.try_into()?;
  Ok(access_request)
}

pub async fn update_access_request_status<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  request_id: Uuid,
  status: AFAccessRequestStatusColumn,
) -> Result<(), AppError> {
  sqlx::query!(
    r#"
      UPDATE af_access_request
      SET status = $2
      WHERE request_id = $1
    "#,
    request_id,
    status as _,
  )
  .execute(executor)
  .await?;
  Ok(())
}

pub async fn delete_access_request<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  request_id: Uuid,
) -> Result<(), AppError> {
  sqlx::query!(
    r#"
      DELETE FROM af_access_request
      WHERE request_id = $1
    "#,
    request_id,
  )
  .execute(executor)
  .await?;
  Ok(())
}
