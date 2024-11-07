use std::sync::Arc;

use app_error::AppError;
use appflowy_collaborate::collab::storage::CollabAccessControlStorage;
use collab::preclude::Collab;
use collab_database::database::DatabaseBody;
use collab_database::entity::FieldType;
use collab_database::workspace_database::NoPersistenceDatabaseCollabService;
use collab_database::workspace_database::WorkspaceDatabaseBody;
use collab_entity::CollabType;
use collab_entity::EncodedCollab;
use collab_folder::SectionItem;
use collab_folder::{CollabOrigin, Folder};
use database::collab::select_workspace_database_oid;
use database::collab::{CollabStorage, GetCollabOrigin};
use database::publish::select_published_view_ids_for_workspace;
use database::publish::select_workspace_id_for_publish_namespace;
use database_entity::dto::QueryCollabResult;
use database_entity::dto::{QueryCollab, QueryCollabParams};
use shared_entity::dto::workspace_dto::AFDatabase;
use shared_entity::dto::workspace_dto::AFDatabaseField;
use shared_entity::dto::workspace_dto::FavoriteFolderView;
use shared_entity::dto::workspace_dto::RecentFolderView;
use shared_entity::dto::workspace_dto::TrashFolderView;
use sqlx::PgPool;
use std::ops::DerefMut;

use anyhow::Context;
use shared_entity::dto::workspace_dto::{FolderView, PublishedView};
use sqlx::types::Uuid;
use std::collections::HashSet;

use tracing::{event, trace};
use validator::Validate;

use access_control::collab::CollabAccessControl;
use database_entity::dto::{
  AFCollabMember, CollabMemberIdentify, InsertCollabMemberParams, QueryCollabMembers,
  UpdateCollabMemberParams,
};

use super::folder_view::collab_folder_to_folder_view;
use super::folder_view::section_items_to_favorite_folder_view;
use super::folder_view::section_items_to_recent_folder_view;
use super::folder_view::section_items_to_trash_folder_view;
use super::publish_outline::collab_folder_to_published_outline;

/// Create a new collab member
/// If the collab member already exists, return [AppError::RecordAlreadyExists]
/// If the collab member does not exist, create a new one
pub async fn create_collab_member(
  pg_pool: &PgPool,
  params: &InsertCollabMemberParams,
  collab_access_control: Arc<dyn CollabAccessControl>,
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
  collab_access_control: Arc<dyn CollabAccessControl>,
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
  collab_access_control: Arc<dyn CollabAccessControl>,
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

pub async fn get_user_favorite_folder_views(
  collab_storage: &CollabAccessControlStorage,
  pg_pool: &PgPool,
  uid: i64,
  workspace_id: Uuid,
) -> Result<Vec<FavoriteFolderView>, AppError> {
  let folder = get_latest_collab_folder(
    collab_storage,
    GetCollabOrigin::User { uid },
    &workspace_id.to_string(),
  )
  .await?;
  let publish_view_ids = select_published_view_ids_for_workspace(pg_pool, workspace_id).await?;
  let publish_view_ids: HashSet<String> = publish_view_ids
    .into_iter()
    .map(|id| id.to_string())
    .collect();
  let deleted_section_item_ids: Vec<String> = folder
    .get_my_trash_sections()
    .iter()
    .map(|s| s.id.clone())
    .collect();
  let favorite_section_items: Vec<SectionItem> = folder
    .get_my_favorite_sections()
    .into_iter()
    .filter(|s| !deleted_section_item_ids.contains(&s.id))
    .collect();
  Ok(section_items_to_favorite_folder_view(
    &favorite_section_items,
    &folder,
    &publish_view_ids,
  ))
}

pub async fn get_user_recent_folder_views(
  collab_storage: &CollabAccessControlStorage,
  pg_pool: &PgPool,
  uid: i64,
  workspace_id: Uuid,
) -> Result<Vec<RecentFolderView>, AppError> {
  let folder = get_latest_collab_folder(
    collab_storage,
    GetCollabOrigin::User { uid },
    &workspace_id.to_string(),
  )
  .await?;
  let deleted_section_item_ids: Vec<String> = folder
    .get_my_trash_sections()
    .iter()
    .map(|s| s.id.clone())
    .collect();
  let recent_section_items: Vec<SectionItem> = folder
    .get_my_recent_sections()
    .into_iter()
    .filter(|s| !deleted_section_item_ids.contains(&s.id))
    .collect();
  let publish_view_ids = select_published_view_ids_for_workspace(pg_pool, workspace_id).await?;
  let publish_view_ids: HashSet<String> = publish_view_ids
    .into_iter()
    .map(|id| id.to_string())
    .collect();
  Ok(section_items_to_recent_folder_view(
    &recent_section_items,
    &folder,
    &publish_view_ids,
  ))
}

pub async fn get_user_trash_folder_views(
  collab_storage: &CollabAccessControlStorage,
  uid: i64,
  workspace_id: Uuid,
) -> Result<Vec<TrashFolderView>, AppError> {
  let folder = get_latest_collab_folder(
    collab_storage,
    GetCollabOrigin::User { uid },
    &workspace_id.to_string(),
  )
  .await?;
  let section_items = folder.get_my_trash_sections();
  Ok(section_items_to_trash_folder_view(&section_items, &folder))
}

pub async fn get_user_workspace_structure(
  collab_storage: &CollabAccessControlStorage,
  pg_pool: &PgPool,
  uid: i64,
  workspace_id: Uuid,
  depth: u32,
  root_view_id: &str,
) -> Result<FolderView, AppError> {
  let depth_limit = 10;
  if depth > depth_limit {
    return Err(AppError::InvalidRequest(format!(
      "Depth {} is too large (limit: {})",
      depth, depth_limit
    )));
  }
  let folder = get_latest_collab_folder(
    collab_storage,
    GetCollabOrigin::User { uid },
    &workspace_id.to_string(),
  )
  .await?;
  let publish_view_ids = select_published_view_ids_for_workspace(pg_pool, workspace_id).await?;
  let publish_view_ids: HashSet<String> = publish_view_ids
    .into_iter()
    .map(|id| id.to_string())
    .collect();
  collab_folder_to_folder_view(root_view_id, &folder, depth, &publish_view_ids)
}

pub async fn get_latest_collab_folder(
  collab_storage: &CollabAccessControlStorage,
  collab_origin: GetCollabOrigin,
  workspace_id: &str,
) -> Result<Folder, AppError> {
  let folder_uid = if let GetCollabOrigin::User { uid } = collab_origin {
    uid
  } else {
    // Dummy uid to open the collab folder if the request does not originate from user
    0
  };
  let encoded_collab = get_latest_collab_encoded(
    collab_storage,
    collab_origin,
    workspace_id,
    workspace_id,
    CollabType::Folder,
  )
  .await?;
  let folder = Folder::from_collab_doc_state(
    folder_uid,
    CollabOrigin::Server,
    encoded_collab.into(),
    workspace_id,
    vec![],
  )
  .map_err(|e| AppError::Unhandled(e.to_string()))?;
  Ok(folder)
}

pub async fn get_latest_collab_encoded(
  collab_storage: &CollabAccessControlStorage,
  collab_origin: GetCollabOrigin,
  workspace_id: &str,
  oid: &str,
  collab_type: CollabType,
) -> Result<EncodedCollab, AppError> {
  collab_storage
    .get_encode_collab(
      collab_origin,
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

pub async fn get_published_view(
  collab_storage: &CollabAccessControlStorage,
  publish_namespace: String,
  pg_pool: &PgPool,
) -> Result<PublishedView, AppError> {
  let workspace_id = select_workspace_id_for_publish_namespace(pg_pool, &publish_namespace).await?;
  let folder = get_latest_collab_folder(
    collab_storage,
    GetCollabOrigin::Server,
    &workspace_id.to_string(),
  )
  .await?;
  let publish_view_ids = select_published_view_ids_for_workspace(pg_pool, workspace_id).await?;
  let publish_view_ids: HashSet<String> = publish_view_ids
    .into_iter()
    .map(|id| id.to_string())
    .collect();
  let published_view: PublishedView =
    collab_folder_to_published_outline(&workspace_id.to_string(), &folder, &publish_view_ids)?;
  Ok(published_view)
}

pub async fn list_database(
  pg_pool: &PgPool,
  collab_storage: &CollabAccessControlStorage,
  uid: i64,
  workspace_uuid_str: String,
) -> Result<Vec<AFDatabase>, AppError> {
  let workspace_uuid: Uuid = workspace_uuid_str.as_str().parse()?;
  let ws_db_oid = select_workspace_database_oid(pg_pool, &workspace_uuid).await?;

  let ec = get_latest_collab_encoded(
    collab_storage,
    GetCollabOrigin::Server,
    &workspace_uuid_str,
    &ws_db_oid,
    CollabType::WorkspaceDatabase,
  )
  .await?;
  let mut collab: Collab =
    Collab::new_with_source(CollabOrigin::Server, &ws_db_oid, ec.into(), vec![], false).map_err(
      |e| {
        AppError::Internal(anyhow::anyhow!(
          "Failed to create collab from encoded collab: {:?}",
          e
        ))
      },
    )?;

  let ws_body = WorkspaceDatabaseBody::open(&mut collab).map_err(|e| {
    AppError::Internal(anyhow::anyhow!(
      "Failed to open workspace database body: {:?}",
      e
    ))
  })?;
  let db_metas = ws_body.get_all_meta(&collab.transact());
  let query_collabs: Vec<QueryCollab> = db_metas
    .into_iter()
    .map(|meta| QueryCollab {
      object_id: meta.database_id.clone(),
      collab_type: CollabType::Database,
    })
    .collect();
  let results = collab_storage
    .batch_get_collab(&uid, query_collabs, true)
    .await;

  let txn = collab.transact();
  let mut af_databases: Vec<AFDatabase> = Vec::with_capacity(results.len());
  for (oid, result) in results {
    match result {
      QueryCollabResult::Success { encode_collab_v1 } => {
        match EncodedCollab::decode_from_bytes(&encode_collab_v1) {
          Ok(ec) => {
            match Collab::new_with_source(CollabOrigin::Server, &oid, ec.into(), vec![], false) {
              Ok(db_collab) => match DatabaseBody::from_collab(
                &db_collab,
                Arc::new(NoPersistenceDatabaseCollabService),
                None,
              ) {
                Some(db_body) => match db_body.metas.get_inline_view_id(&txn) {
                  Some(iid) => match db_body.views.get_view(&txn, &iid) {
                    Some(iview) => {
                      let name = iview.name;

                      let db_fields = db_body.fields.get_all_fields(&txn);
                      let mut af_fields: Vec<AFDatabaseField> = Vec::with_capacity(db_fields.len());
                      for db_field in db_fields {
                        af_fields.push(AFDatabaseField {
                          name: db_field.name,
                          field_type: format!("{:?}", FieldType::from(db_field.field_type)),
                        });
                      }
                      af_databases.push(AFDatabase {
                        id: db_body.get_database_id(&txn),
                        name,
                        fields: af_fields,
                      });
                    },
                    None => tracing::warn!("Failed to get inline view: {}", iid),
                  },
                  None => tracing::error!("Failed to get inline view id for database: {}", oid),
                },
                None => tracing::error!("Failed to create db_body from db_collab, oid: {}", oid),
              },
              Err(err) => tracing::error!("Failed to create db_collab: {:?}", err),
            }
          },
          Err(err) => tracing::error!("Failed to decode collab: {:?}", err),
        }
      },
      QueryCollabResult::Failed { error } => {
        tracing::warn!("Failed to get collab: {:?}", error)
      },
    }
  }

  Ok(af_databases)
}
