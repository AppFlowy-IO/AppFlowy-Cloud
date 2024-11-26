use std::collections::HashMap;
use std::sync::Arc;

use app_error::AppError;
use appflowy_collaborate::collab::storage::CollabAccessControlStorage;
use chrono::DateTime;
use chrono::Utc;
use collab::core::collab::DataSource;
use collab::preclude::Collab;
use collab_database::database::DatabaseBody;
use collab_database::entity::FieldType;
use collab_database::fields::Field;
use collab_database::fields::TypeOptions;
use collab_database::rows::RowDetail;
use collab_database::workspace_database::NoPersistenceDatabaseCollabService;
use collab_database::workspace_database::WorkspaceDatabase;
use collab_database::workspace_database::WorkspaceDatabaseBody;
use collab_entity::CollabType;
use collab_entity::EncodedCollab;
use collab_folder::SectionItem;
use collab_folder::{CollabOrigin, Folder};
use database::collab::select_last_updated_database_row_ids;
use database::collab::select_workspace_database_oid;
use database::collab::{CollabStorage, GetCollabOrigin};
use database::publish::select_published_view_ids_for_workspace;
use database::publish::select_workspace_id_for_publish_namespace;
use database_entity::dto::QueryCollabResult;
use database_entity::dto::{QueryCollab, QueryCollabParams};
use shared_entity::dto::workspace_dto::AFDatabase;
use shared_entity::dto::workspace_dto::AFDatabaseField;
use shared_entity::dto::workspace_dto::AFDatabaseRow;
use shared_entity::dto::workspace_dto::AFDatabaseRowDetail;
use shared_entity::dto::workspace_dto::DatabaseRowUpdatedItem;
use shared_entity::dto::workspace_dto::FavoriteFolderView;
use shared_entity::dto::workspace_dto::FolderViewMinimal;
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
use super::folder_view::to_dto_folder_view_miminal;
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

pub async fn get_latest_workspace_database(
  collab_storage: &CollabAccessControlStorage,
  pg_pool: &PgPool,
  collab_origin: GetCollabOrigin,
  workspace_id: Uuid,
) -> Result<(String, WorkspaceDatabase), AppError> {
  let workspace_database_oid = select_workspace_database_oid(pg_pool, &workspace_id).await?;
  let workspace_database_collab = {
    let encoded_collab = get_latest_collab_encoded(
      collab_storage,
      collab_origin,
      &workspace_id.to_string(),
      &workspace_database_oid,
      CollabType::WorkspaceDatabase,
    )
    .await?;
    collab_from_doc_state(encoded_collab.doc_state.to_vec(), &workspace_database_oid)?
  };

  let workspace_database = WorkspaceDatabase::open(workspace_database_collab)
    .map_err(|err| AppError::Unhandled(format!("failed to open workspace database: {}", err)))?;
  Ok((workspace_database_oid, workspace_database))
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

pub async fn get_latest_collab(
  storage: &CollabAccessControlStorage,
  origin: GetCollabOrigin,
  workspace_id: &str,
  oid: &str,
  collab_type: CollabType,
) -> Result<Collab, AppError> {
  let ec = get_latest_collab_encoded(storage, origin, workspace_id, oid, collab_type).await?;
  let collab: Collab = Collab::new_with_source(CollabOrigin::Server, oid, ec.into(), vec![], false)
    .map_err(|e| {
      AppError::Internal(anyhow::anyhow!(
        "Failed to create collab from encoded collab: {:?}",
        e
      ))
    })?;
  Ok(collab)
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

  let mut ws_body_collab = get_latest_collab(
    collab_storage,
    GetCollabOrigin::Server,
    &workspace_uuid_str,
    &ws_db_oid,
    CollabType::WorkspaceDatabase,
  )
  .await?;

  let ws_body = WorkspaceDatabaseBody::open(&mut ws_body_collab).map_err(|e| {
    AppError::Internal(anyhow::anyhow!(
      "Failed to open workspace database body: {:?}",
      e
    ))
  })?;
  let db_metas = ws_body.get_all_meta(&ws_body_collab.transact());

  let folder = get_latest_collab_folder(
    collab_storage,
    GetCollabOrigin::User { uid },
    &workspace_uuid_str,
  )
  .await?;

  let mut af_databases = Vec::with_capacity(db_metas.len());
  for db_meta in db_metas {
    let id = db_meta.database_id;
    let views: Vec<FolderViewMinimal> = db_meta
      .linked_views
      .into_iter()
      .map(|view_id| {
        folder
          .get_view(&view_id)
          .map(|view| to_dto_folder_view_miminal(&view))
          .unwrap_or_default()
      })
      .collect();
    af_databases.push(AFDatabase { id, views });
  }

  Ok(af_databases)
}

pub async fn list_database_row_ids(
  collab_storage: &CollabAccessControlStorage,
  workspace_uuid_str: &str,
  database_uuid_str: &str,
) -> Result<Vec<AFDatabaseRow>, AppError> {
  let (db_collab, db_body) =
    get_database_body(collab_storage, workspace_uuid_str, database_uuid_str).await?;
  // get any view_id
  let txn = db_collab.transact();
  let iid = db_body.get_inline_view_id(&txn);

  let iview = db_body.views.get_view(&txn, &iid).ok_or_else(|| {
    AppError::Internal(anyhow::anyhow!("Failed to get inline view, iid: {}", iid))
  })?;

  let db_rows = iview
    .row_orders
    .into_iter()
    .map(|row_order| AFDatabaseRow {
      id: row_order.id.to_string(),
    })
    .collect();

  Ok(db_rows)
}

pub async fn get_database_fields(
  collab_storage: &CollabAccessControlStorage,
  workspace_uuid_str: &str,
  database_uuid_str: &str,
) -> Result<Vec<AFDatabaseField>, AppError> {
  let (db_collab, db_body) =
    get_database_body(collab_storage, workspace_uuid_str, database_uuid_str).await?;

  let all_fields = db_body.fields.get_all_fields(&db_collab.transact());
  let mut acc = Vec::with_capacity(all_fields.len());
  for field in all_fields {
    acc.push(AFDatabaseField {
      id: field.id,
      name: field.name,
      field_type: format!("{:?}", FieldType::from(field.field_type)),
      is_primary: field.is_primary,
    });
  }
  Ok(acc)
}

pub async fn list_database_row_ids_updated(
  collab_storage: &CollabAccessControlStorage,
  pg_pool: &PgPool,
  workspace_uuid_str: &str,
  database_uuid_str: &str,
  after: &DateTime<Utc>,
) -> Result<Vec<DatabaseRowUpdatedItem>, AppError> {
  let row_ids = list_database_row_ids(collab_storage, workspace_uuid_str, database_uuid_str)
    .await?
    .into_iter()
    .map(|row| row.id)
    .collect::<Vec<String>>();

  let workspace_uuid: Uuid = workspace_uuid_str.parse()?;
  let updated_row_ids =
    select_last_updated_database_row_ids(pg_pool, &workspace_uuid, &row_ids, after).await?;
  Ok(updated_row_ids)
}

pub async fn list_database_row_details(
  collab_storage: &CollabAccessControlStorage,
  uid: i64,
  workspace_uuid_str: String,
  database_uuid_str: String,
  row_ids: &[&str],
) -> Result<Vec<AFDatabaseRowDetail>, AppError> {
  let query_collabs: Vec<QueryCollab> = row_ids
    .iter()
    .map(|id| QueryCollab {
      object_id: id.to_string(),
      collab_type: CollabType::DatabaseRow,
    })
    .collect();

  let database_collab = get_latest_collab(
    collab_storage,
    GetCollabOrigin::User { uid },
    &workspace_uuid_str,
    &database_uuid_str,
    CollabType::Database,
  )
  .await?;
  let db_body = DatabaseBody::from_collab(
    &database_collab,
    Arc::new(NoPersistenceDatabaseCollabService),
    None,
  )
  .ok_or_else(|| {
    AppError::Internal(anyhow::anyhow!(
      "Failed to create database body from collab, db_collab_id: {}",
      database_uuid_str,
    ))
  })?;

  // create a map of field id to field.
  // ensure that the field name is unique.
  // if the field name is repeated, it will be appended with the field id,
  // under practical usage circumstances, no other collision should occur
  let field_by_id: HashMap<String, Field> = {
    let all_fields = db_body.fields.get_all_fields(&database_collab.transact());

    let mut uniq_name_set: HashSet<String> = HashSet::with_capacity(all_fields.len());
    let mut field_by_id: HashMap<String, Field> = HashMap::with_capacity(all_fields.len());

    for mut field in all_fields {
      // if the name already exists, append the field id to the name
      if uniq_name_set.contains(&field.name) {
        let new_name = format!("{}-{}", field.name, field.id);
        field.name = new_name.clone();
      }
      uniq_name_set.insert(field.name.clone());
      field_by_id.insert(field.id.clone(), field);
    }
    field_by_id
  };

  let mut selection_name_by_id: HashMap<String, String> = HashMap::new();
  for field in field_by_id.values() {
    add_to_selection_from_field(&mut selection_name_by_id, field);
  }

  let database_row_details = collab_storage
    .batch_get_collab(&uid, query_collabs, true)
    .await
    .into_iter()
    .flat_map(|(id, result)| match result {
      QueryCollabResult::Success { encode_collab_v1 } => {
        let ec = EncodedCollab::decode_from_bytes(&encode_collab_v1).unwrap();
        let collab =
          Collab::new_with_source(CollabOrigin::Server, &id, ec.into(), vec![], false).unwrap();
        let row_detail = RowDetail::from_collab(&collab).unwrap();
        let cells = convert_database_cells_human_readable(
          row_detail.row.cells,
          &field_by_id,
          &selection_name_by_id,
        );
        Some(AFDatabaseRowDetail { id, cells })
      },
      QueryCollabResult::Failed { error } => {
        tracing::warn!("Failed to get collab: {:?}", error);
        None
      },
    })
    .collect::<Vec<AFDatabaseRowDetail>>();

  Ok(database_row_details)
}

fn convert_database_cells_human_readable(
  db_cells: HashMap<String, HashMap<String, yrs::Any>>,
  field_by_id: &HashMap<String, Field>,
  selection_name_by_id: &HashMap<String, String>,
) -> HashMap<String, HashMap<String, serde_json::Value>> {
  let mut human_readable_records: HashMap<String, HashMap<String, serde_json::Value>> =
    HashMap::with_capacity(db_cells.len());

  for (field_id, cell) in db_cells {
    let field = match field_by_id.get(&field_id) {
      Some(field) => field,
      None => {
        tracing::error!("Failed to get field by id: {}", field_id);
        continue;
      },
    };
    let field_type = FieldType::from(field.field_type);

    let mut human_readable_cell: HashMap<String, serde_json::Value> =
      HashMap::with_capacity(cell.len());
    for (key, value) in cell {
      let serde_value: serde_json::Value = match key.as_str() {
        "created_at" | "last_modified" => match value.cast::<i64>() {
          Ok(timestamp) => chrono::DateTime::from_timestamp(timestamp, 0)
            .unwrap_or_default()
            .to_rfc3339()
            .into(),
          Err(err) => {
            tracing::error!("Failed to cast timestamp: {:?}", err);
            serde_json::Value::Null
          },
        },
        "field_type" => format!("{:?}", field_type).into(),
        "data" => {
          match field_type {
            FieldType::DateTime => {
              if let yrs::any::Any::String(value_str) = value {
                let int_value = value_str.parse::<i64>().unwrap_or_default();
                chrono::DateTime::from_timestamp(int_value, 0)
                  .unwrap_or_default()
                  .to_rfc3339()
                  .into()
              } else {
                serde_json::to_value(value).unwrap_or_default()
              }
            },
            FieldType::Checklist => {
              if let yrs::any::Any::String(value_str) = value {
                serde_json::from_str(&value_str).unwrap_or_default()
              } else {
                serde_json::to_value(value).unwrap_or_default()
              }
            },
            FieldType::Media => {
              if let yrs::any::Any::Array(arr) = value {
                let mut acc = Vec::with_capacity(arr.len());
                for v in arr.as_ref() {
                  if let yrs::any::Any::String(value_str) = v {
                    let serde_value = serde_json::from_str(value_str).unwrap_or_default();
                    acc.push(serde_value);
                  }
                }
                serde_json::Value::Array(acc)
              } else {
                serde_json::to_value(value).unwrap_or_default()
              }
            },
            FieldType::SingleSelect => {
              if let yrs::any::Any::String(ref value_str) = value {
                selection_name_by_id
                  .get(value_str.as_ref())
                  .map(|v| v.to_string())
                  .map(serde_json::Value::String)
                  .unwrap_or_else(|| value.to_string().into())
              } else {
                serde_json::to_value(value).unwrap_or_default()
              }
            },
            FieldType::MultiSelect => {
              if let yrs::any::Any::String(value_str) = value {
                value_str
                  .split(',')
                  .filter_map(|v| selection_name_by_id.get(v).map(|v| v.to_string()))
                  .fold(String::new(), |mut acc, s| {
                    if !acc.is_empty() {
                      acc.push(',');
                    }
                    acc.push_str(&s);
                    acc
                  })
                  .into()
              } else {
                serde_json::to_value(value).unwrap_or_default()
              }
            },
            // Handle different field types formatting as needed
            _ => serde_json::to_value(value).unwrap_or_default(),
          }
        },
        _ => serde_json::to_value(value).unwrap_or_default(),
      };
      human_readable_cell.insert(key, serde_value);
    }
    human_readable_records.insert(field.name.clone(), human_readable_cell);
  }
  human_readable_records
}

fn add_to_selection_from_field(name_by_id: &mut HashMap<String, String>, field: &Field) {
  let field_type = FieldType::from(field.field_type);
  match field_type {
    FieldType::SingleSelect => {
      add_to_selection_from_type_options(name_by_id, &field.type_options, &field_type);
    },
    FieldType::MultiSelect => {
      add_to_selection_from_type_options(name_by_id, &field.type_options, &field_type)
    },
    _ => (),
  }
}

fn add_to_selection_from_type_options(
  name_by_id: &mut HashMap<String, String>,
  type_options: &TypeOptions,
  field_type: &FieldType,
) {
  if let Some(type_opt) = type_options.get(&field_type.type_id()) {
    if let Some(yrs::Any::String(arc_str)) = type_opt.get("content") {
      if let Ok(serde_value) = serde_json::from_str::<serde_json::Value>(arc_str) {
        if let Some(selections) = serde_value.get("options").and_then(|v| v.as_array()) {
          for selection in selections {
            if let serde_json::Value::Object(selection) = selection {
              if let (Some(id), Some(name)) = (
                selection.get("id").and_then(|v| v.as_str()),
                selection.get("name").and_then(|v| v.as_str()),
              ) {
                name_by_id.insert(id.to_owned(), name.to_owned());
              }
            }
          }
        }
      }
    }
  };
}

async fn get_database_body(
  collab_storage: &CollabAccessControlStorage,
  workspace_uuid_str: &str,
  database_uuid_str: &str,
) -> Result<(Collab, DatabaseBody), AppError> {
  let db_collab = get_latest_collab(
    collab_storage,
    GetCollabOrigin::Server,
    workspace_uuid_str,
    database_uuid_str,
    CollabType::Database,
  )
  .await?;
  let db_body = DatabaseBody::from_collab(
    &db_collab,
    Arc::new(NoPersistenceDatabaseCollabService),
    None,
  )
  .ok_or_else(|| {
    AppError::Internal(anyhow::anyhow!(
      "Failed to create database body from collab, db_collab_id: {}",
      database_uuid_str,
    ))
  })?;
  Ok((db_collab, db_body))
}

pub fn collab_from_doc_state(doc_state: Vec<u8>, object_id: &str) -> Result<Collab, AppError> {
  let collab = Collab::new_with_source(
    CollabOrigin::Server,
    object_id,
    DataSource::DocStateV1(doc_state),
    vec![],
    false,
  )
  .map_err(|e| AppError::Unhandled(e.to_string()))?;
  Ok(collab)
}
