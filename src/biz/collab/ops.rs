use std::cell::OnceCell;
use std::collections::HashMap;
use std::sync::Arc;

use app_error::AppError;
use appflowy_collaborate::collab::storage::CollabAccessControlStorage;
use chrono::DateTime;
use chrono::Utc;
use collab::preclude::Collab;
use collab_database::database::gen_field_id;
use collab_database::database::gen_row_id;
use collab_database::entity::FieldType;
use collab_database::fields::timestamp_type_option::TimestampTypeOption;
use collab_database::fields::Field;
use collab_database::fields::TypeOptionCellWriter;
use collab_database::fields::TypeOptions;
use collab_database::rows::Cell;
use collab_database::rows::CreateRowParams;
use collab_database::rows::DatabaseRowBody;
use collab_database::rows::Row;
use collab_database::rows::RowDetail;
use collab_database::views::OrderObjectPosition;
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
use database_entity::dto::CollabParams;
use database_entity::dto::QueryCollab;
use database_entity::dto::QueryCollabResult;
use shared_entity::dto::workspace_dto::AFDatabase;
use shared_entity::dto::workspace_dto::AFDatabaseField;
use shared_entity::dto::workspace_dto::AFDatabaseRow;
use shared_entity::dto::workspace_dto::AFDatabaseRowDetail;
use shared_entity::dto::workspace_dto::AFInsertDatabaseField;
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

use crate::biz::collab::utils::field_by_name_uniq;
use crate::biz::workspace::ops::broadcast_update;

use super::folder_view::collab_folder_to_folder_view;
use super::folder_view::section_items_to_favorite_folder_view;
use super::folder_view::section_items_to_recent_folder_view;
use super::folder_view::section_items_to_trash_folder_view;
use super::folder_view::to_dto_folder_view_miminal;
use super::publish_outline::collab_folder_to_published_outline;
use super::utils::collab_from_doc_state;
use super::utils::collab_to_bin;
use super::utils::field_by_id_name_uniq;
use super::utils::get_database_body;
use super::utils::get_latest_collab;
use super::utils::get_latest_collab_encoded;
use super::utils::get_row_details_serde;
use super::utils::type_option_reader_by_id;
use super::utils::type_option_writer_by_id;
use super::utils::type_options_serde;

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
  collab_folder_to_folder_view(
    workspace_id,
    root_view_id,
    &folder,
    depth,
    &publish_view_ids,
  )
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

  let trash = folder
    .get_all_trash_sections()
    .into_iter()
    .map(|s| s.id)
    .collect::<HashSet<_>>();

  let mut af_databases = Vec::with_capacity(db_metas.len());
  for db_meta in db_metas {
    let id = db_meta.database_id;
    let mut views: Vec<FolderViewMinimal> = Vec::new();
    for linked_view_id in db_meta.linked_views {
      if !trash.contains(&linked_view_id) {
        if let Some(folder_view) = folder.get_view(&linked_view_id) {
          views.push(to_dto_folder_view_miminal(&folder_view));
        };
      }
    }
    if !views.is_empty() {
      af_databases.push(AFDatabase { id, views });
    }
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

pub async fn insert_database_row(
  collab_storage: &CollabAccessControlStorage,
  pg_pool: &PgPool,
  workspace_uuid_str: &str,
  database_uuid_str: &str,
  uid: i64,
  cell_value_by_id: HashMap<String, serde_json::Value>,
) -> Result<String, AppError> {
  // get database types and type options
  let (mut db_collab, db_body) =
    get_database_body(collab_storage, workspace_uuid_str, database_uuid_str).await?;

  let all_fields = db_body.fields.get_all_fields(&db_collab.transact());
  let field_by_id = all_fields.iter().fold(HashMap::new(), |mut acc, field| {
    acc.insert(field.id.clone(), field.clone());
    acc
  });
  let type_option_reader_by_id = type_option_writer_by_id(&all_fields);
  let field_by_name = field_by_name_uniq(all_fields);

  let new_db_row_id = gen_row_id();
  let mut new_db_row_collab =
    Collab::new_with_origin(CollabOrigin::Empty, new_db_row_id.clone(), vec![], false);

  let new_db_row_body = {
    let database_body = DatabaseRowBody::create(
      new_db_row_id.clone(),
      &mut new_db_row_collab,
      Row::empty(new_db_row_id.clone(), database_uuid_str),
    );
    let mut txn = new_db_row_collab.transact_mut();

    // Create a OnceCell to hold the timestamp
    let timestamp_cell: OnceCell<String> = OnceCell::new();
    let get_timestamp = || timestamp_cell.get_or_init(|| Utc::now().timestamp().to_string());

    // Insert `created_at` and `modified_at` fields (if any)
    for (field_id, field) in &field_by_name {
      let field_type = FieldType::from(field.field_type);
      match field_type {
        FieldType::LastEditedTime | FieldType::CreatedTime => {
          let new_cell = match type_option_reader_by_id.get(field_id.as_str()) {
            Some(cell_writer) => cell_writer.convert_json_to_cell(get_timestamp().as_str().into()),
            None => {
              TimestampTypeOption::default().convert_json_to_cell(get_timestamp().as_str().into())
            },
          };
          database_body.update(&mut txn, |row_update| {
            row_update.update_cells(|cells_update| {
              cells_update.insert_cell(&field.id, new_cell);
            });
          });
        },
        _ => {},
      }
    }

    for (id, serde_val) in cell_value_by_id {
      let field = match field_by_id.get(&id) {
        Some(f) => f,
        // try use field name if id not found
        None => match field_by_name.get(&id) {
          Some(f) => f,
          None => {
            tracing::warn!(
              "field not found: {} for database: {}",
              id,
              database_uuid_str
            );
            continue;
          },
        },
      };
      let cell_writer = match type_option_reader_by_id.get(&field.id) {
        Some(cell_writer) => cell_writer,
        None => {
          tracing::error!("Failed to get type option writer for field: {}", field.id);
          continue;
        },
      };
      let new_cell: Cell = cell_writer.convert_json_to_cell(serde_val);
      database_body.update(&mut txn, |row_update| {
        row_update.update_cells(|cells_update| {
          cells_update.insert_cell(&field.id, new_cell);
        });
      });
    }
    database_body
  };

  // Create new row order
  let ts_now = chrono::Utc::now().timestamp();
  let row_order = db_body
    .create_row(CreateRowParams {
      id: new_db_row_id.clone(),
      database_id: database_uuid_str.to_string(),
      cells: new_db_row_body
        .cells(&new_db_row_collab.transact())
        .unwrap_or_default(),
      height: 30,
      visibility: true,
      row_position: OrderObjectPosition::End,
      created_at: ts_now,
      modified_at: ts_now,
    })
    .await
    .map_err(|e| AppError::Internal(anyhow::anyhow!("Failed to create row: {:?}", e)))?;

  // Prepare new row collab binary to store in postgres
  let db_row_ec_v1 = collab_to_bin(new_db_row_collab, CollabType::DatabaseRow).await?;

  // For each database view, add the new row order
  let db_collab_update = {
    let mut txn = db_collab.transact_mut();
    let mut db_views = db_body.views.get_all_views(&txn);
    for db_view in db_views.iter_mut() {
      db_view.row_orders.push(row_order.clone());
    }
    db_body.views.clear(&mut txn);
    for view in db_views {
      db_body.views.insert_view(&mut txn, view);
    }

    txn.encode_update_v1()
  };
  let updated_db_collab = collab_to_bin(db_collab, CollabType::Database).await?;

  let mut db_txn = pg_pool.begin().await?;
  // insert row
  collab_storage
    .upsert_new_collab_with_transaction(
      workspace_uuid_str,
      &uid,
      CollabParams {
        object_id: new_db_row_id.to_string(),
        encoded_collab_v1: db_row_ec_v1.into(),
        collab_type: CollabType::DatabaseRow,
        embeddings: None,
      },
      &mut db_txn,
      "inserting new database row from server",
    )
    .await?;

  // update database
  collab_storage
    .upsert_new_collab_with_transaction(
      workspace_uuid_str,
      &uid,
      CollabParams {
        object_id: database_uuid_str.to_string(),
        encoded_collab_v1: updated_db_collab.into(),
        collab_type: CollabType::Database,
        embeddings: None,
      },
      &mut db_txn,
      "inserting updated database from server",
    )
    .await?;

  db_txn.commit().await?;
  broadcast_update(collab_storage, database_uuid_str, db_collab_update).await?;
  Ok(new_db_row_id.to_string())
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
    let field_type = FieldType::from(field.field_type);
    acc.push(AFDatabaseField {
      id: field.id,
      name: field.name,
      field_type: format!("{:?}", field_type),
      type_option: type_options_serde(&field.type_options, &field_type),
      is_primary: field.is_primary,
    });
  }
  Ok(acc)
}

// inserts a new field into the database
// returns the id of the field created
pub async fn add_database_field(
  uid: i64,
  collab_storage: &CollabAccessControlStorage,
  pg_pool: &PgPool,
  workspace_id: &str,
  database_id: &str,
  insert_field: AFInsertDatabaseField,
) -> Result<String, AppError> {
  let (mut db_collab, db_body) =
    get_database_body(collab_storage, workspace_id, database_id).await?;

  let new_id = gen_field_id();
  let mut type_options = TypeOptions::new();
  let type_option_data = insert_field
    .type_option_data
    .unwrap_or(serde_json::json!({}));
  match serde_json::from_value(type_option_data) {
    Ok(tod) => type_options.insert(insert_field.field_type.to_string(), tod),
    Err(err) => {
      return Err(AppError::InvalidRequest(format!(
        "Failed to parse type option: {:?}",
        err
      )));
    },
  };

  let new_field = Field {
    id: new_id.clone(),
    name: insert_field.name,
    field_type: insert_field.field_type,
    type_options,
    ..Default::default()
  };

  let db_collab_update = {
    let mut yrs_txn = db_collab.transact_mut();
    db_body.create_field(
      &mut yrs_txn,
      None,
      new_field,
      &OrderObjectPosition::End,
      &HashMap::new(),
    );
    yrs_txn.encode_update_v1()
  };
  let updated_db_collab = collab_to_bin(db_collab, CollabType::Database).await?;

  let mut pg_txn = pg_pool.begin().await?;
  collab_storage
    .upsert_new_collab_with_transaction(
      workspace_id,
      &uid,
      CollabParams {
        object_id: database_id.to_string(),
        encoded_collab_v1: updated_db_collab.into(),
        collab_type: CollabType::Database,
        embeddings: None,
      },
      &mut pg_txn,
      "inserting updated database from server",
    )
    .await?;

  pg_txn.commit().await?;
  broadcast_update(collab_storage, database_id, db_collab_update).await?;

  Ok(new_id)
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
  unsupported_field_types: &[FieldType],
) -> Result<Vec<AFDatabaseRowDetail>, AppError> {
  let (database_collab, db_body) =
    get_database_body(collab_storage, &workspace_uuid_str, &database_uuid_str).await?;

  let all_fields: Vec<Field> = db_body
    .fields
    .get_all_fields(&database_collab.transact())
    .into_iter()
    .filter(|field| !unsupported_field_types.contains(&FieldType::from(field.field_type)))
    .collect();
  if all_fields.is_empty() {
    return Ok(vec![]);
  }

  let type_option_reader_by_id = type_option_reader_by_id(&all_fields);
  let field_by_id = field_by_id_name_uniq(all_fields);
  let query_collabs: Vec<QueryCollab> = row_ids
    .iter()
    .map(|id| QueryCollab {
      object_id: id.to_string(),
      collab_type: CollabType::DatabaseRow,
    })
    .collect();
  let database_row_details = collab_storage
    .batch_get_collab(&uid, &workspace_uuid_str, query_collabs, true)
    .await
    .into_iter()
    .flat_map(|(id, result)| match result {
      QueryCollabResult::Success { encode_collab_v1 } => {
        let ec = match EncodedCollab::decode_from_bytes(&encode_collab_v1) {
          Ok(ec) => ec,
          Err(err) => {
            tracing::error!("Failed to decode encoded collab: {:?}", err);
            return None;
          },
        };
        let collab =
          match Collab::new_with_source(CollabOrigin::Server, &id, ec.into(), vec![], false) {
            Ok(collab) => collab,
            Err(err) => {
              tracing::error!("Failed to create collab: {:?}", err);
              return None;
            },
          };
        let row_detail = match RowDetail::from_collab(&collab) {
          Some(row_detail) => row_detail,
          None => {
            tracing::error!("Failed to get row detail from collab: {:?}", collab);
            return None;
          },
        };
        let cells = get_row_details_serde(row_detail, &field_by_id, &type_option_reader_by_id);
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
