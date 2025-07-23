use std::collections::HashMap;

use app_error::AppError;
use chrono::DateTime;
use chrono::Utc;
use collab::preclude::Collab;
use collab_database::database::gen_field_id;
use collab_database::entity::FieldType;
use collab_database::fields::Field;
use collab_database::fields::TypeOptions;
use collab_database::rows::meta_id_from_row_id;
use collab_database::rows::CreateRowParams;
use collab_database::rows::DatabaseRowBody;
use collab_database::rows::Row;
use collab_database::rows::RowDetail;
use collab_database::rows::RowId;
use collab_database::rows::RowMetaKey;
use collab_database::views::OrderObjectPosition;
use collab_database::workspace_database::WorkspaceDatabase;
use collab_database::workspace_database::WorkspaceDatabaseBody;
use collab_document::document::Document;
use collab_entity::CollabType;
use collab_entity::EncodedCollab;
use collab_folder::hierarchy_builder::NestedChildViewBuilder;
use collab_folder::Folder;
use collab_folder::SectionItem;
use collab_folder::{CollabOrigin, SpaceInfo};
use collab_rt_entity::user::RealtimeUser;
use database::collab::select_last_updated_database_row_ids;
use database::collab::select_workspace_database_oid;
use database::collab::{CollabStore, GetCollabOrigin};
use database::publish::select_published_view_ids_for_workspace;
use database::publish::select_published_view_ids_with_publish_info_for_workspace;
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
use shared_entity::dto::workspace_dto::PublishedViewInfo;
use shared_entity::dto::workspace_dto::RecentFolderView;
use shared_entity::dto::workspace_dto::TrashFolderView;
use sqlx::PgPool;
use yrs::Map;

use super::folder_view::collab_folder_to_folder_view;
use super::folder_view::section_items_to_favorite_folder_view;
use super::folder_view::section_items_to_recent_folder_view;
use super::folder_view::section_items_to_trash_folder_view;
use super::folder_view::to_dto_folder_view_miminal;
use super::publish_outline::collab_folder_to_published_outline;
use super::utils::collab_to_bin;
use super::utils::create_row_document;
use super::utils::field_by_id_name_uniq;
use super::utils::get_latest_collab;
use super::utils::get_latest_collab_database_body;
use super::utils::get_latest_collab_database_row_body;
use super::utils::get_row_details_serde;
use super::utils::type_option_reader_by_id;
use super::utils::type_options_serde;
use super::utils::write_to_database_row;
use super::utils::CreatedRowDocument;
use super::utils::DocChanges;
use super::utils::DEFAULT_SPACE_ICON;
use super::utils::DEFAULT_SPACE_ICON_COLOR;
use crate::api::metrics::AppFlowyWebMetrics;
use crate::biz::collab::folder_view::check_if_view_is_space;
use crate::biz::collab::utils::get_database_row_doc_changes;
use crate::biz::workspace::page_view::update_workspace_folder_data;
use crate::state::AppState;
use appflowy_collaborate::ws2::{CollabUpdatePublisher, WorkspaceCollabInstanceCache};
use collab::core::collab::{default_client_id, CollabOptions};
use shared_entity::dto::workspace_dto::{FolderView, PublishedView};
use sqlx::types::Uuid;
use std::collections::HashSet;
use std::sync::Arc;
use yrs::block::ClientID;

pub async fn get_user_favorite_folder_views(
  collab_instance_cache: &impl WorkspaceCollabInstanceCache,
  pg_pool: &PgPool,
  uid: i64,
  workspace_id: Uuid,
) -> Result<Vec<FavoriteFolderView>, AppError> {
  let folder = collab_instance_cache.get_folder(workspace_id).await?;
  let publish_view_ids = select_published_view_ids_for_workspace(pg_pool, workspace_id).await?;
  let publish_view_ids: HashSet<String> = publish_view_ids
    .into_iter()
    .map(|id| id.to_string())
    .collect();
  let deleted_section_item_ids: Vec<String> = folder
    .get_my_trash_sections(uid)
    .iter()
    .map(|s| s.id.clone())
    .collect();
  let favorite_section_items: Vec<SectionItem> = folder
    .get_my_favorite_sections(uid)
    .into_iter()
    .filter(|s| !deleted_section_item_ids.contains(&s.id))
    .collect();
  Ok(section_items_to_favorite_folder_view(
    &favorite_section_items,
    &folder,
    &publish_view_ids,
    uid,
  ))
}

pub async fn get_user_recent_folder_views(
  collab_instance_cache: &impl WorkspaceCollabInstanceCache,
  pg_pool: &PgPool,
  uid: i64,
  workspace_id: Uuid,
) -> Result<Vec<RecentFolderView>, AppError> {
  let folder = collab_instance_cache.get_folder(workspace_id).await?;
  let deleted_section_item_ids: Vec<String> = folder
    .get_my_trash_sections(uid)
    .iter()
    .map(|s| s.id.clone())
    .collect();
  let recent_section_items: Vec<SectionItem> = folder
    .get_my_recent_sections(uid)
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
    uid,
  ))
}

pub async fn get_user_trash_folder_views(
  collab_instance_cache: &impl WorkspaceCollabInstanceCache,
  uid: i64,
  workspace_id: Uuid,
) -> Result<Vec<TrashFolderView>, AppError> {
  let folder = collab_instance_cache.get_folder(workspace_id).await?;
  let section_items = folder.get_my_trash_sections(uid);
  Ok(section_items_to_trash_folder_view(
    &section_items,
    &folder,
    uid,
  ))
}

#[allow(clippy::too_many_arguments)]
fn patch_old_workspace_folder(
  user: RealtimeUser,
  workspace_id: &str,
  folder: &mut Folder,
  child_view_id_without_space: &[String],
) -> Result<Vec<u8>, AppError> {
  let encoded_update = {
    let space_id = Uuid::new_v4().to_string();

    let space_view = NestedChildViewBuilder::new(user.uid, workspace_id.to_string())
      .with_view_id(space_id.clone())
      .with_name("General")
      .with_extra(|extra| {
        extra
          .with_space_info(SpaceInfo {
            space_icon: Some(DEFAULT_SPACE_ICON.to_string()),
            space_icon_color: Some(DEFAULT_SPACE_ICON_COLOR.to_string()),
            ..Default::default()
          })
          .build()
      })
      .build()
      .view;
    let mut txn = folder.collab.transact_mut();
    folder
      .body
      .views
      .insert(&mut txn, space_view, None, user.uid);
    for (i, current_view_id) in child_view_id_without_space.iter().enumerate() {
      let previous_view_id = if i == 0 {
        None
      } else {
        Some(child_view_id_without_space[i - 1].clone())
      };
      folder.body.move_nested_view(
        &mut txn,
        current_view_id,
        &space_id,
        previous_view_id,
        user.uid,
      );
    }
    txn.encode_update_v1()
  };
  Ok(encoded_update)
}

async fn fix_old_workspace_folder(
  appflowy_web_metrics: &AppFlowyWebMetrics,
  update_publisher: &impl CollabUpdatePublisher,
  user: RealtimeUser,
  mut folder: Folder,
  workspace_id: Uuid,
) -> Result<Folder, AppError> {
  let root_view = folder
    .get_view(&workspace_id.to_string(), user.uid)
    .ok_or_else(|| {
      AppError::InvalidRequest(format!(
        "Failed to get view for workspace_id: {}",
        workspace_id
      ))
    })?;
  let direct_workspace_children: Vec<String> = root_view
    .children
    .iter()
    .map(|view_id| view_id.to_string())
    .collect();
  let has_at_least_one_space = direct_workspace_children
    .iter()
    .filter_map(|view_id| folder.get_view(view_id, user.uid))
    .any(|view| check_if_view_is_space(&view));
  if !has_at_least_one_space {
    let folder_update = patch_old_workspace_folder(
      user.clone(),
      &workspace_id.to_string(),
      &mut folder,
      &direct_workspace_children,
    )?;
    update_workspace_folder_data(
      appflowy_web_metrics,
      update_publisher,
      user,
      workspace_id,
      folder_update,
    )
    .await?;
  }
  Ok(folder)
}

#[allow(clippy::too_many_arguments)]
pub async fn get_user_workspace_structure(
  state: &AppState,
  user: RealtimeUser,
  workspace_id: Uuid,
  depth: u32,
  root_view_id: &Uuid,
) -> Result<FolderView, AppError> {
  let appflowy_web_metrics = &state.metrics.appflowy_web_metrics;
  let depth_limit = 10;
  if depth > depth_limit {
    return Err(AppError::InvalidRequest(format!(
      "Depth {} is too large (limit: {})",
      depth, depth_limit
    )));
  }
  let folder = state.ws_server.get_folder(workspace_id).await?;
  let patched_folder = fix_old_workspace_folder(
    appflowy_web_metrics,
    &state.ws_server,
    user.clone(),
    folder,
    workspace_id,
  )
  .await?;

  let publish_view_ids =
    select_published_view_ids_for_workspace(&state.pg_pool, workspace_id).await?;
  let publish_view_ids: HashSet<_> = publish_view_ids.into_iter().collect();
  collab_folder_to_folder_view(
    workspace_id,
    root_view_id,
    &patched_folder,
    depth,
    &publish_view_ids,
    user.uid,
  )
}

pub async fn get_latest_workspace_database(
  collab_storage: &Arc<dyn CollabStore>,
  pg_pool: &PgPool,
  collab_origin: GetCollabOrigin,
  workspace_id: Uuid,
) -> Result<(Uuid, WorkspaceDatabase), AppError> {
  let workspace_database_oid = select_workspace_database_oid(pg_pool, &workspace_id).await?;
  let workspace_database_collab = get_latest_collab(
    collab_storage,
    collab_origin,
    workspace_id,
    workspace_database_oid,
    CollabType::WorkspaceDatabase,
    default_client_id(),
  )
  .await?;

  let workspace_database = WorkspaceDatabase::open(workspace_database_collab)
    .map_err(|err| AppError::Unhandled(format!("failed to open workspace database: {}", err)))?;
  Ok((workspace_database_oid, workspace_database))
}

pub async fn get_published_view(
  collab_instance_cache: &impl WorkspaceCollabInstanceCache,
  publish_namespace: String,
  pg_pool: &PgPool,
  uid: i64,
) -> Result<PublishedView, AppError> {
  let workspace_id = select_workspace_id_for_publish_namespace(pg_pool, &publish_namespace).await?;
  let folder = collab_instance_cache.get_folder(workspace_id).await?;
  let publish_view_ids_with_publish_info =
    select_published_view_ids_with_publish_info_for_workspace(pg_pool, workspace_id).await?;
  let publish_view_id_to_info_map: HashMap<String, PublishedViewInfo> =
    publish_view_ids_with_publish_info
      .into_iter()
      .map(|pv| {
        (
          pv.view_id.to_string(),
          PublishedViewInfo {
            publisher_email: pv.publisher_email.clone(),
            publish_name: pv.publish_name.clone(),
            publish_timestamp: pv.publish_timestamp,
            comments_enabled: pv.comments_enabled,
            duplicate_enabled: pv.duplicate_enabled,
          },
        )
      })
      .collect();

  let published_view: PublishedView = collab_folder_to_published_outline(
    &workspace_id.to_string(),
    &folder,
    &publish_view_id_to_info_map,
    uid,
  )?;
  Ok(published_view)
}

pub async fn list_database(
  pg_pool: &PgPool,
  collab_instance_cache: &impl WorkspaceCollabInstanceCache,
  collab_storage: &Arc<dyn CollabStore>,
  uid: i64,
  workspace_id: Uuid,
) -> Result<Vec<AFDatabase>, AppError> {
  let ws_db_oid = select_workspace_database_oid(pg_pool, &workspace_id).await?;

  let mut ws_body_collab = get_latest_collab(
    collab_storage,
    GetCollabOrigin::Server,
    workspace_id,
    ws_db_oid,
    CollabType::WorkspaceDatabase,
    default_client_id(),
  )
  .await?;

  let ws_body = WorkspaceDatabaseBody::open(&mut ws_body_collab).map_err(|e| {
    AppError::Internal(anyhow::anyhow!(
      "Failed to open workspace database body: {:?}",
      e
    ))
  })?;
  let db_metas = ws_body.get_all_meta(&ws_body_collab.transact());

  let folder = collab_instance_cache.get_folder(workspace_id).await?;
  let trash = folder
    .get_all_trash_sections(uid)
    .into_iter()
    .map(|s| s.id)
    .collect::<HashSet<_>>();

  let mut af_databases = Vec::with_capacity(db_metas.len());
  for db_meta in db_metas {
    let id = db_meta.database_id;
    let mut views: Vec<FolderViewMinimal> = Vec::new();
    for linked_view_id in db_meta.linked_views {
      if !trash.contains(&linked_view_id) {
        if let Some(folder_view) = folder.get_view(&linked_view_id, uid) {
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
  collab_storage: &Arc<dyn CollabStore>,
  workspace_uuid: Uuid,
  database_uuid: Uuid,
) -> Result<Vec<AFDatabaseRow>, AppError> {
  let (db_collab, db_body) =
    get_latest_collab_database_body(collab_storage, workspace_uuid, database_uuid).await?;
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

#[allow(clippy::too_many_arguments)]
pub async fn insert_database_row(
  state: &AppState,
  workspace_uuid: Uuid,
  database_uuid: Uuid,
  uid: i64,
  new_db_row_id: Option<Uuid>,
  cell_value_by_id: HashMap<String, serde_json::Value>,
  row_doc_content: Option<String>,
) -> Result<String, AppError> {
  let new_db_row_id = new_db_row_id.unwrap_or_else(Uuid::new_v4);
  let new_db_row_id_str = RowId::from(new_db_row_id.to_string());
  let creation_time = Utc::now();
  let client_id = default_client_id();

  let options = CollabOptions::new(new_db_row_id.to_string(), client_id);
  let mut new_db_row_collab = Collab::new_with_options(CollabOrigin::Empty, options)
    .map_err(|err| AppError::Internal(err.into()))?;
  let new_db_row_body = DatabaseRowBody::create(
    new_db_row_id_str.clone(),
    &mut new_db_row_collab,
    Row::empty(new_db_row_id_str, &database_uuid.to_string()),
  );
  new_db_row_body.update(&mut new_db_row_collab.transact_mut(), |row_update| {
    row_update.set_created_at(Utc::now().timestamp());
  });

  let new_row_doc_creation: Option<(Uuid, CreatedRowDocument)> = match row_doc_content {
    Some(row_doc_content) if !row_doc_content.is_empty() => {
      // update row to indicate that the document is not empty
      let is_document_empty_id = meta_id_from_row_id(&new_db_row_id, RowMetaKey::IsDocumentEmpty);
      new_db_row_body.get_meta().insert(
        &mut new_db_row_collab.transact_mut(),
        is_document_empty_id,
        false,
      );

      // get document id
      let new_doc_id = new_db_row_body
        .document_id(&new_db_row_collab.transact())
        .map_err(|err| AppError::Internal(anyhow::anyhow!("Failed to get document id: {:?}", err)))?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Failed to get document id")))?;

      let new_doc_id = Uuid::parse_str(&new_doc_id)?;
      let created_row_doc = create_row_document(
        workspace_uuid,
        uid,
        new_doc_id,
        &state.ws_server,
        row_doc_content,
      )
      .await?;
      Some((new_doc_id, created_row_doc))
    },
    _ => None,
  };

  let (mut db_collab, db_body) =
    get_latest_collab_database_body(&state.collab_storage, workspace_uuid, database_uuid).await?;
  write_to_database_row(
    &db_body,
    &mut new_db_row_collab.transact_mut(),
    &new_db_row_body,
    cell_value_by_id,
    creation_time.timestamp(),
  )
  .await?;

  // Create new row order
  let ts_now = creation_time.timestamp();
  let row_order = db_body
    .create_row(
      CreateRowParams {
        id: new_db_row_id.to_string().into(),
        database_id: database_uuid.to_string(),
        cells: new_db_row_body
          .cells(&new_db_row_collab.transact())
          .unwrap_or_default(),
        height: 30,
        visibility: true,
        row_position: OrderObjectPosition::End,
        created_at: ts_now,
        modified_at: ts_now,
      },
      client_id,
    )
    .await
    .map_err(|e| AppError::Internal(anyhow::anyhow!("Failed to create row: {:?}", e)))?;

  let new_db_row_ec_v1 = collab_to_bin(new_db_row_collab, CollabType::DatabaseRow).await?;

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

  state
    .ws_server
    .publish_update(
      workspace_uuid,
      database_uuid,
      CollabType::Database,
      &CollabOrigin::Server,
      db_collab_update.clone(),
    )
    .await?;

  let collab_storage = state.collab_storage.clone();
  let mut db_txn = state.pg_pool.begin().await?;
  // handle row document (if provided)
  if let Some((doc_id, created_doc)) = new_row_doc_creation {
    state
      .ws_server
      .publish_update(
        workspace_uuid,
        workspace_uuid,
        CollabType::Folder,
        &CollabOrigin::Server,
        created_doc.folder_updates,
      )
      .await?;

    // insert document
    collab_storage
      .upsert_new_collab_with_transaction(
        workspace_uuid,
        &uid,
        CollabParams {
          object_id: doc_id,
          encoded_collab_v1: created_doc.doc_ec_bytes.into(),
          collab_type: CollabType::Document,
          updated_at: None,
        },
        &mut db_txn,
        "inserting new database row document from server",
      )
      .await?;
  };

  // insert row
  collab_storage
    .upsert_new_collab_with_transaction(
      workspace_uuid,
      &uid,
      CollabParams {
        object_id: new_db_row_id,
        encoded_collab_v1: new_db_row_ec_v1.into(),
        collab_type: CollabType::DatabaseRow,
        updated_at: None,
      },
      &mut db_txn,
      "inserting new database row from server",
    )
    .await?;

  db_txn.commit().await?;
  Ok(new_db_row_id.to_string())
}

#[allow(clippy::too_many_arguments)]
pub async fn upsert_database_row(
  state: &AppState,
  workspace_uuid: Uuid,
  database_uuid: Uuid,
  uid: i64,
  row_id: Uuid,
  cell_value_by_id: HashMap<String, serde_json::Value>,
  row_doc_content: Option<String>,
) -> Result<(), AppError> {
  let collab_storage = &state.collab_storage;
  let (mut db_row_collab, db_row_body) =
    match get_latest_collab_database_row_body(collab_storage, workspace_uuid, row_id).await {
      Ok(res) => res,
      Err(err) => match err {
        AppError::RecordNotFound(_) => {
          return insert_database_row(
            state,
            workspace_uuid,
            database_uuid,
            uid,
            Some(row_id),
            cell_value_by_id,
            row_doc_content,
          )
          .await
          .map(|_id| {});
        },
        _ => return Err(err),
      },
    };

  // At this point, db row exists,
  // so we modify it, put into storage and broadcast change
  let (_db_collab, db_body) =
    get_latest_collab_database_body(collab_storage, workspace_uuid, database_uuid).await?;

  let mut db_row_txn = db_row_collab.transact_mut();
  write_to_database_row(
    &db_body,
    &mut db_row_txn,
    &db_row_body,
    cell_value_by_id,
    Utc::now().timestamp(),
  )
  .await?;

  // determine if there are any document changes
  let doc_changes: Option<(Uuid, DocChanges)> = get_database_row_doc_changes(
    collab_storage,
    &state.ws_server,
    workspace_uuid,
    row_doc_content,
    &db_row_body,
    &mut db_row_txn,
    &row_id,
    uid,
  )
  .await?;

  // finalize update for database row
  let db_row_collab_updates = db_row_txn.encode_update_v1();
  drop(db_row_txn);
  state
    .ws_server
    .publish_update(
      workspace_uuid,
      row_id,
      CollabType::DatabaseRow,
      &CollabOrigin::Server,
      db_row_collab_updates,
    )
    .await?;

  let db_row_ec_v1 = collab_to_bin(db_row_collab, CollabType::DatabaseRow).await?;
  // write to disk and broadcast changes
  let mut db_txn = state.pg_pool.begin().await?;
  collab_storage
    .upsert_new_collab_with_transaction(
      workspace_uuid,
      &uid,
      CollabParams {
        object_id: row_id,
        encoded_collab_v1: db_row_ec_v1.into(),
        collab_type: CollabType::DatabaseRow,
        updated_at: None,
      },
      &mut db_txn,
      "inserting new database row from server",
    )
    .await?;

  // handle document changes
  if let Some((doc_id, doc_changes)) = doc_changes {
    match doc_changes {
      DocChanges::Update(updated_doc, doc_update) => {
        collab_storage
          .upsert_new_collab_with_transaction(
            workspace_uuid,
            &uid,
            CollabParams {
              object_id: doc_id,
              encoded_collab_v1: updated_doc.into(),
              collab_type: CollabType::Document,
              updated_at: None,
            },
            &mut db_txn,
            "updating database row document from server",
          )
          .await?;

        state
          .ws_server
          .publish_update(
            workspace_uuid,
            doc_id,
            CollabType::Document,
            &CollabOrigin::Server,
            doc_update,
          )
          .await?;
      },
      DocChanges::Insert(created_doc) => {
        let CreatedRowDocument {
          folder_updates,
          doc_ec_bytes,
        } = created_doc;

        state
          .ws_server
          .publish_update(
            workspace_uuid,
            workspace_uuid,
            CollabType::Folder,
            &CollabOrigin::Server,
            folder_updates,
          )
          .await?;

        // insert document
        collab_storage
          .upsert_new_collab_with_transaction(
            workspace_uuid,
            &uid,
            CollabParams {
              object_id: doc_id,
              encoded_collab_v1: doc_ec_bytes.into(),
              collab_type: CollabType::Document,
              updated_at: None,
            },
            &mut db_txn,
            "inserting new database row document from server",
          )
          .await?;
      },
    }
  }

  db_txn.commit().await?;
  Ok(())
}

pub async fn get_database_fields(
  collab_storage: &Arc<dyn CollabStore>,
  workspace_uuid: Uuid,
  database_uuid: Uuid,
) -> Result<Vec<AFDatabaseField>, AppError> {
  let (db_collab, db_body) =
    get_latest_collab_database_body(collab_storage, workspace_uuid, database_uuid).await?;

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
  state: &AppState,
  workspace_id: Uuid,
  database_id: Uuid,
  insert_field: AFInsertDatabaseField,
) -> Result<String, AppError> {
  let (mut db_collab, db_body) =
    get_latest_collab_database_body(&state.collab_storage, workspace_id, database_id).await?;

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

  state
    .ws_server
    .publish_update(
      workspace_id,
      database_id,
      CollabType::Database,
      &CollabOrigin::Server,
      db_collab_update,
    )
    .await?;

  Ok(new_id)
}

pub async fn list_database_row_ids_updated(
  collab_storage: &Arc<dyn CollabStore>,
  pg_pool: &PgPool,
  workspace_uuid: Uuid,
  database_uuid: Uuid,
  after: &DateTime<Utc>,
) -> Result<Vec<DatabaseRowUpdatedItem>, AppError> {
  let row_ids: Vec<_> = list_database_row_ids(collab_storage, workspace_uuid, database_uuid)
    .await?
    .into_iter()
    .flat_map(|row| Uuid::parse_str(&row.id))
    .collect();

  let updated_row_ids =
    select_last_updated_database_row_ids(pg_pool, &workspace_uuid, &row_ids, after).await?;
  Ok(updated_row_ids)
}

pub async fn list_database_row_details(
  collab_storage: &Arc<dyn CollabStore>,
  uid: i64,
  workspace_uuid: Uuid,
  database_uuid: Uuid,
  row_ids: &[Uuid],
  unsupported_field_types: &[FieldType],
  with_doc: bool,
) -> Result<Vec<AFDatabaseRowDetail>, AppError> {
  let (database_collab, db_body) =
    get_latest_collab_database_body(collab_storage, workspace_uuid, database_uuid).await?;

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
  let client_id = default_client_id();
  let query_collabs: Vec<QueryCollab> = row_ids
    .iter()
    .map(|id| QueryCollab {
      object_id: *id,
      collab_type: CollabType::DatabaseRow,
    })
    .collect();
  let mut db_row_details = collab_storage
    .batch_get_collab(&uid, workspace_uuid, query_collabs)
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
        let id = id.to_string();
        let options = collab::core::collab::CollabOptions::new(id.to_string(), client_id)
          .with_data_source(ec.into());
        let collab = match Collab::new_with_options(CollabOrigin::Server, options) {
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

        let has_doc = !row_detail.meta.is_document_empty;
        let cells = get_row_details_serde(row_detail, &field_by_id, &type_option_reader_by_id);
        Some(AFDatabaseRowDetail {
          id,
          cells,
          has_doc,
          doc: None,
        })
      },
      QueryCollabResult::Failed { error } => {
        tracing::warn!("Failed to get collab: {:?}", error);
        None
      },
    })
    .collect::<Vec<AFDatabaseRowDetail>>();

  // Fill in the document content if requested and exists
  if with_doc {
    let doc_id_by_row_id = db_row_details
      .iter()
      .filter(|row| row.has_doc)
      .flat_map(|row| {
        row.id.parse::<Uuid>().ok().map(|row_uuid| {
          (
            row_uuid,
            meta_id_from_row_id(&row_uuid, RowMetaKey::DocumentId)
              .parse::<Uuid>()
              .unwrap(),
          )
        })
      })
      .collect::<HashMap<_, _>>();
    let query_db_docs = doc_id_by_row_id
      .values()
      .map(|doc_id| QueryCollab {
        object_id: *doc_id,
        collab_type: CollabType::Document,
      })
      .collect::<Vec<_>>();
    let mut query_res = collab_storage
      .batch_get_collab(&uid, workspace_uuid, query_db_docs)
      .await;
    for row_detail in &mut db_row_details {
      if let Err(err) = fill_in_db_row_doc(client_id, row_detail, &doc_id_by_row_id, &mut query_res)
      {
        tracing::error!("Failed to fill in document content: {:?}", err);
      };
    }
  }

  Ok(db_row_details)
}

fn fill_in_db_row_doc(
  client_id: ClientID,
  row_detail: &mut AFDatabaseRowDetail,
  doc_id_by_row_id: &HashMap<Uuid, Uuid>,
  query_res: &mut HashMap<Uuid, QueryCollabResult>,
) -> Result<(), AppError> {
  let doc_id = doc_id_by_row_id
    .get(&row_detail.id.parse()?)
    .ok_or_else(|| {
      AppError::Internal(anyhow::anyhow!(
        "Failed to get document id for row id: {}",
        row_detail.id
      ))
    })?;
  let res = query_res.remove(doc_id).ok_or_else(|| {
    AppError::Internal(anyhow::anyhow!(
      "Failed to get document collab for row id: {}",
      row_detail.id
    ))
  })?;

  let ec_bytes = match res {
    QueryCollabResult::Success { encode_collab_v1 } => encode_collab_v1,
    QueryCollabResult::Failed { error } => return Err(AppError::Internal(anyhow::anyhow!(error))),
  };
  let ec = EncodedCollab::decode_from_bytes(&ec_bytes)?;
  let options = collab::core::collab::CollabOptions::new(doc_id.to_string(), client_id)
    .with_data_source(ec.into());
  let doc_collab = Collab::new_with_options(CollabOrigin::Server, options).map_err(|err| {
    AppError::Internal(anyhow::anyhow!(
      "Failed to create document collab: {:?}",
      err
    ))
  })?;
  let doc = Document::open(doc_collab)
    .map_err(|err| AppError::Internal(anyhow::anyhow!("Failed to open document: {:?}", err)))?;
  let plain_text = doc.paragraphs().join("");
  row_detail.doc = Some(plain_text);
  Ok(())
}
