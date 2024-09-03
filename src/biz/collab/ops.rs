use std::sync::Arc;

use app_error::AppError;
use appflowy_collaborate::collab::storage::CollabAccessControlStorage;
use collab_entity::CollabType;
use collab_entity::EncodedCollab;
use collab_folder::{CollabOrigin, Folder};
use database::collab::{CollabStorage, GetCollabOrigin};
use database_entity::dto::QueryCollab;
use database_entity::dto::QueryCollabParams;
use sqlx::PgPool;
use std::ops::DerefMut;

use anyhow::Context;
use shared_entity::dto::workspace_dto::FolderView;
use sqlx::types::Uuid;
use tracing::{event, trace};
use validator::Validate;

use access_control::collab::CollabAccessControl;
use database_entity::dto::{
  AFCollabMember, CollabMemberIdentify, InsertCollabMemberParams, QueryCollabMembers,
  UpdateCollabMemberParams,
};

use super::folder_view::collab_folder_to_folder_view;

/// Create a new collab member
/// If the collab member already exists, return [AppError::RecordAlreadyExists]
/// If the collab member does not exist, create a new one
pub async fn create_collab_member(
  pg_pool: &PgPool,
  params: &InsertCollabMemberParams,
  collab_access_control: &impl CollabAccessControl,
) -> Result<(), AppError> {
  params.validate()?;

  let mut transaction = pg_pool
    .begin()
    .await
    .context("acquire transaction to insert collab member")?;

  if database::collab::is_collab_member_exists(
    params.uid,
    &params.object_id,
    transaction.deref_mut(),
  )
  .await?
  {
    return Err(AppError::RecordAlreadyExists(format!(
      "Collab member with uid {} and object_id {} already exists",
      params.uid, params.object_id
    )));
  }

  trace!("Inserting collab member: {:?}", params);
  database::collab::insert_collab_member(
    params.uid,
    &params.object_id,
    &params.access_level,
    &mut transaction,
  )
  .await?;

  collab_access_control
    .update_access_level_policy(&params.uid, &params.object_id, params.access_level)
    .await?;

  transaction
    .commit()
    .await
    .context("fail to commit the transaction to insert collab member")?;
  Ok(())
}

pub async fn upsert_collab_member(
  pg_pool: &PgPool,
  _user_uuid: &Uuid,
  params: &UpdateCollabMemberParams,
  collab_access_control: &impl CollabAccessControl,
) -> Result<(), AppError> {
  params.validate()?;
  let mut transaction = pg_pool
    .begin()
    .await
    .context("acquire transaction to upsert collab member")?;

  collab_access_control
    .update_access_level_policy(&params.uid, &params.object_id, params.access_level)
    .await?;

  database::collab::insert_collab_member(
    params.uid,
    &params.object_id,
    &params.access_level,
    &mut transaction,
  )
  .await?;

  transaction
    .commit()
    .await
    .context("fail to commit the transaction to upsert collab member")?;
  Ok(())
}

pub async fn get_collab_member(
  pg_pool: &PgPool,
  params: &CollabMemberIdentify,
) -> Result<AFCollabMember, AppError> {
  params.validate()?;
  let collab_member =
    database::collab::select_collab_member(&params.uid, &params.object_id, pg_pool).await?;
  Ok(collab_member)
}

pub async fn delete_collab_member(
  pg_pool: &PgPool,
  params: &CollabMemberIdentify,
  collab_access_control: &impl CollabAccessControl,
) -> Result<(), AppError> {
  params.validate()?;
  let mut transaction = pg_pool
    .begin()
    .await
    .context("acquire transaction to remove collab member")?;
  event!(
    tracing::Level::DEBUG,
    "Deleting member:{} from {}",
    params.uid,
    params.object_id
  );
  database::collab::delete_collab_member(params.uid, &params.object_id, &mut transaction).await?;

  collab_access_control
    .remove_access_level(&params.uid, &params.object_id)
    .await?;

  transaction
    .commit()
    .await
    .context("fail to commit the transaction to remove collab member")?;
  Ok(())
}

pub async fn get_collab_member_list(
  pg_pool: &PgPool,
  params: &QueryCollabMembers,
) -> Result<Vec<AFCollabMember>, AppError> {
  params.validate()?;
  let collab_member = database::collab::select_collab_members(&params.object_id, pg_pool).await?;
  Ok(collab_member)
}

pub async fn get_user_workspace_structure(
  collab_storage: Arc<CollabAccessControlStorage>,
  uid: i64,
  workspace_id: String,
  depth: u32,
) -> Result<FolderView, AppError> {
  let depth_limit = 10;
  if depth > depth_limit {
    return Err(AppError::InvalidRequest(format!(
      "Depth {} is too large (limit: {})",
      depth, depth_limit
    )));
  }
  let folder = get_latest_collab_folder(collab_storage, &uid, &workspace_id).await?;
  let folder_view: FolderView = collab_folder_to_folder_view(&folder, depth);
  Ok(folder_view)
}

pub async fn get_latest_collab_folder(
  collab_storage: Arc<CollabAccessControlStorage>,
  uid: &i64,
  workspace_id: &str,
) -> Result<Folder, AppError> {
  let encoded_collab = get_latest_collab_encoded(
    collab_storage,
    uid,
    workspace_id,
    workspace_id,
    CollabType::Folder,
  )
  .await?;
  let folder = Folder::from_collab_doc_state(
    uid,
    CollabOrigin::Server,
    encoded_collab.into(),
    workspace_id,
    vec![],
  )
  .map_err(|e| AppError::Unhandled(e.to_string()))?;
  Ok(folder)
}

pub async fn get_latest_collab_encoded(
  collab_storage: Arc<CollabAccessControlStorage>,
  uid: &i64,
  workspace_id: &str,
  oid: &str,
  collab_type: CollabType,
) -> Result<EncodedCollab, AppError> {
  collab_storage
    .get_encode_collab(
      GetCollabOrigin::User { uid: *uid },
      QueryCollabParams {
        workspace_id: workspace_id.to_string(),
        inner: QueryCollab {
          object_id: oid.to_string(),
          collab_type,
        },
      },
      true,
    )
    .await
}
