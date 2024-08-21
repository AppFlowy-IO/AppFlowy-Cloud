use app_error::AppError;
use database_entity::dto::{PublishCollabItem, PublishInfo};
use sqlx::PgPool;
use uuid::Uuid;

use database::{
  publish::{
    delete_published_collabs, insert_or_replace_publish_collabs, select_publish_collab_meta,
    select_published_collab_blob, select_published_collab_info,
    select_user_is_collab_publisher_for_all_views, select_workspace_publish_namespace,
    select_workspace_publish_namespace_exists, update_workspace_publish_namespace,
  },
  workspace::select_user_is_workspace_owner,
};

use super::ops::check_workspace_owner;

pub async fn publish_collabs(
  pg_pool: &PgPool,
  workspace_id: &Uuid,
  publisher_uuid: &Uuid,
  publish_items: Vec<PublishCollabItem<serde_json::Value, Vec<u8>>>,
) -> Result<(), AppError> {
  for publish_item in &publish_items {
    check_collab_publish_name(publish_item.meta.publish_name.as_str())?;
  }
  insert_or_replace_publish_collabs(pg_pool, workspace_id, publisher_uuid, publish_items).await?;
  Ok(())
}

pub async fn get_published_collab(
  pg_pool: &PgPool,
  publish_namespace: &str,
  publish_name: &str,
) -> Result<serde_json::Value, AppError> {
  let metadata = select_publish_collab_meta(pg_pool, publish_namespace, publish_name).await?;
  Ok(metadata)
}

pub async fn get_published_collab_blob(
  pg_pool: &PgPool,
  publish_namespace: &str,
  publish_name: &str,
) -> Result<Vec<u8>, AppError> {
  select_published_collab_blob(pg_pool, publish_namespace, publish_name).await
}

pub async fn get_published_collab_info(
  pg_pool: &PgPool,
  view_id: &Uuid,
) -> Result<PublishInfo, AppError> {
  select_published_collab_info(pg_pool, view_id).await
}

pub async fn delete_published_workspace_collab(
  pg_pool: &PgPool,
  workspace_id: &Uuid,
  view_ids: &[Uuid],
  user_uuid: &Uuid,
) -> Result<(), AppError> {
  check_workspace_owner_or_publisher(pg_pool, user_uuid, workspace_id, view_ids).await?;
  delete_published_collabs(pg_pool, workspace_id, view_ids).await?;
  Ok(())
}

async fn check_workspace_owner_or_publisher(
  pg_pool: &PgPool,
  user_uuid: &Uuid,
  workspace_id: &Uuid,
  view_id: &[Uuid],
) -> Result<(), AppError> {
  let is_owner = select_user_is_workspace_owner(pg_pool, user_uuid, workspace_id).await?;
  if !is_owner {
    let is_publisher =
      select_user_is_collab_publisher_for_all_views(pg_pool, user_uuid, workspace_id, view_id)
        .await?;
    if !is_publisher {
      return Err(AppError::UserUnAuthorized(
        "User is not the owner of the workspace or the publisher of the document".to_string(),
      ));
    }
  }
  Ok(())
}

fn check_collab_publish_name(publish_name: &str) -> Result<(), AppError> {
  // Check len
  if publish_name.len() > 128 {
    return Err(AppError::InvalidRequest(
      "Publish name must be at most 128 characters long".to_string(),
    ));
  }

  // Only contain alphanumeric characters and hyphens
  for c in publish_name.chars() {
    if !c.is_alphanumeric() && c != '-' {
      return Err(AppError::InvalidRequest(
        "Publish name must only contain alphanumeric characters and hyphens".to_string(),
      ));
    }
  }

  Ok(())
}

pub async fn set_workspace_namespace(
  pg_pool: &PgPool,
  user_uuid: &Uuid,
  workspace_id: &Uuid,
  new_namespace: &str,
) -> Result<(), AppError> {
  check_workspace_owner(pg_pool, user_uuid, workspace_id).await?;
  check_workspace_namespace(new_namespace).await?;
  if select_workspace_publish_namespace_exists(pg_pool, workspace_id, new_namespace).await? {
    return Err(AppError::PublishNamespaceAlreadyTaken(
      "publish namespace is already taken".to_string(),
    ));
  };
  update_workspace_publish_namespace(pg_pool, workspace_id, new_namespace).await?;
  Ok(())
}

pub async fn get_workspace_publish_namespace(
  pg_pool: &PgPool,
  workspace_id: &Uuid,
) -> Result<String, AppError> {
  select_workspace_publish_namespace(pg_pool, workspace_id).await
}

async fn check_workspace_namespace(new_namespace: &str) -> Result<(), AppError> {
  // Check len
  if new_namespace.len() < 8 {
    return Err(AppError::InvalidRequest(
      "Namespace must be at least 8 characters long".to_string(),
    ));
  }

  if new_namespace.len() > 64 {
    return Err(AppError::InvalidRequest(
      "Namespace must be at most 32 characters long".to_string(),
    ));
  }

  // Only contain alphanumeric characters and hyphens
  for c in new_namespace.chars() {
    if !c.is_alphanumeric() && c != '-' {
      return Err(AppError::InvalidRequest(
        "Namespace must only contain alphanumeric characters and hyphens".to_string(),
      ));
    }
  }

  // TODO: add more checks for reserved words

  Ok(())
}
