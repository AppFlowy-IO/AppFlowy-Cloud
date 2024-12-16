use super::ops::broadcast_update;
use crate::api::metrics::AppFlowyWebMetrics;
use crate::api::ws::RealtimeServerAddr;
use crate::biz::collab::folder_view::{
  check_if_view_is_space, parse_extra_field_as_json, to_dto_view_icon, to_dto_view_layout,
  to_folder_view_icon, to_space_permission,
};
use crate::biz::collab::ops::{get_latest_collab_folder, get_latest_workspace_database};
use crate::biz::collab::utils::{collab_from_doc_state, get_latest_collab_encoded};
use actix_web::web::Data;
use anyhow::anyhow;
use app_error::AppError;
use appflowy_collaborate::actix_ws::entities::ClientHttpUpdateMessage;
use appflowy_collaborate::collab::storage::CollabAccessControlStorage;
use bytes::Bytes;
use chrono::DateTime;
use collab::core::collab::Collab;
use collab_database::database::{
  gen_database_group_id, gen_database_id, gen_field_id, gen_row_id, Database, DatabaseContext,
};
use collab_database::entity::{CreateDatabaseParams, CreateViewParams, EncodedDatabase, FieldType};
use collab_database::fields::select_type_option::{
  SelectOption, SelectOptionColor, SelectOptionIds, SingleSelectTypeOption,
};
use collab_database::fields::{default_field_settings_for_fields, Field};
use collab_database::rows::{new_cell_builder, CreateRowParams};
use collab_database::template::entity::CELL_DATA;
use collab_database::views::{
  BoardLayoutSetting, CalendarLayoutSetting, DatabaseLayout, Group, GroupSetting, GroupSettingMap,
  LayoutSetting, LayoutSettings,
};
use collab_database::workspace_database::{NoPersistenceDatabaseCollabService, WorkspaceDatabase};
use collab_database::{database::DatabaseBody, rows::RowId};
use collab_document::document::Document;
use collab_document::document_data::default_document_data;
use collab_entity::{CollabType, EncodedCollab};
use collab_folder::hierarchy_builder::NestedChildViewBuilder;
use collab_folder::{timestamp, CollabOrigin, Folder};
use collab_rt_entity::user::RealtimeUser;
use database::collab::{select_workspace_database_oid, CollabStorage, GetCollabOrigin};
use database::publish::select_published_view_ids_for_workspace;
use database::user::select_web_user_from_uid;
use database_entity::dto::{CollabParams, QueryCollab, QueryCollabResult};
use itertools::Itertools;
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use serde_json::json;
use shared_entity::dto::workspace_dto::{
  FolderView, Page, PageCollab, PageCollabData, Space, SpacePermission, ViewIcon, ViewLayout,
};
use sqlx::{PgPool, Transaction};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;
use tracing::instrument;
use uuid::Uuid;

struct WorkspaceDatabaseUpdate {
  pub updated_encoded_collab: Vec<u8>,
  pub encoded_updates: Vec<u8>,
}

struct FolderUpdate {
  pub updated_encoded_collab: Vec<u8>,
  pub encoded_update: Vec<u8>,
}

#[allow(clippy::too_many_arguments)]
pub async fn update_space(
  pg_pool: &PgPool,
  collab_storage: &CollabAccessControlStorage,
  uid: i64,
  workspace_id: Uuid,
  view_id: &str,
  space_permission: &SpacePermission,
  name: &str,
  space_icon: &str,
  space_icon_color: &str,
) -> Result<(), AppError> {
  let collab_origin = GetCollabOrigin::User { uid };
  let mut folder =
    get_latest_collab_folder(collab_storage, collab_origin, &workspace_id.to_string()).await?;
  let folder_update = update_space_properties(
    view_id,
    &mut folder,
    space_permission,
    name,
    space_icon,
    space_icon_color,
  )
  .await?;
  let mut transaction = pg_pool.begin().await?;
  insert_and_broadcast_workspace_folder_update(
    uid,
    workspace_id,
    folder_update,
    collab_storage,
    &mut transaction,
  )
  .await?;
  transaction.commit().await?;
  Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn create_space(
  pg_pool: &PgPool,
  collab_storage: &CollabAccessControlStorage,
  uid: i64,
  workspace_id: Uuid,
  space_permission: &SpacePermission,
  name: &str,
  space_icon: &str,
  space_color: &str,
) -> Result<Space, AppError> {
  let default_document_collab_params = prepare_default_document_collab_param()?;
  let view_id = default_document_collab_params.object_id.clone();
  let collab_origin = GetCollabOrigin::User { uid };
  let mut folder =
    get_latest_collab_folder(collab_storage, collab_origin, &workspace_id.to_string()).await?;
  let folder_update = add_new_space_to_folder(
    uid,
    &workspace_id.to_string(),
    &view_id,
    &mut folder,
    space_permission,
    name,
    space_icon,
    space_color,
  )
  .await?;
  let mut transaction = pg_pool.begin().await?;
  let start = Instant::now();
  let action = format!("Create new space: {}", view_id);
  collab_storage
    .upsert_new_collab_with_transaction(
      &workspace_id.to_string(),
      &uid,
      default_document_collab_params,
      &mut transaction,
      &action,
    )
    .await?;
  insert_and_broadcast_workspace_folder_update(
    uid,
    workspace_id,
    folder_update,
    collab_storage,
    &mut transaction,
  )
  .await?;
  transaction.commit().await?;
  collab_storage.metrics().observe_pg_tx(start.elapsed());
  Ok(Space { view_id })
}

pub async fn create_page(
  pg_pool: &PgPool,
  collab_storage: &CollabAccessControlStorage,
  uid: i64,
  workspace_id: Uuid,
  parent_view_id: &str,
  view_layout: &ViewLayout,
  name: Option<&str>,
) -> Result<Page, AppError> {
  match view_layout {
    ViewLayout::Document => {
      create_document_page(
        pg_pool,
        collab_storage,
        uid,
        workspace_id,
        parent_view_id,
        name,
      )
      .await
    },
    ViewLayout::Grid => {
      create_grid_page(
        pg_pool,
        collab_storage,
        uid,
        workspace_id,
        parent_view_id,
        name,
      )
      .await
    },
    ViewLayout::Calendar => {
      create_calendar_page(
        pg_pool,
        collab_storage,
        uid,
        workspace_id,
        parent_view_id,
        name,
      )
      .await
    },
    ViewLayout::Board => {
      create_board_page(
        pg_pool,
        collab_storage,
        uid,
        workspace_id,
        parent_view_id,
        name,
      )
      .await
    },
    layout => Err(AppError::InvalidRequest(format!(
      "The layout type {} is not supported for page creation",
      layout
    ))),
  }
}

fn prepare_default_document_collab_param() -> Result<CollabParams, AppError> {
  let object_id = Uuid::new_v4().to_string();
  let document_data = default_document_data(&object_id);
  let document = Document::create(&object_id, document_data)
    .map_err(|err| AppError::Internal(anyhow!("Failed to create default document: {}", err)))?;
  let encoded_collab_v1 = document
    .encode_collab()
    .map_err(|err| AppError::Internal(anyhow!("Failed to encode default document: {}", err)))?
    .encode_to_bytes()?;
  Ok(CollabParams {
    object_id: object_id.clone(),
    encoded_collab_v1: encoded_collab_v1.into(),
    collab_type: CollabType::Document,
  })
}

#[allow(clippy::too_many_arguments)]
async fn prepare_new_encoded_database(
  view_id: &str,
  database_id: &str,
  name: &str,
  fields: Vec<Field>,
  rows: Vec<CreateRowParams>,
  database_layout: DatabaseLayout,
  layout_setting: Option<LayoutSetting>,
  group_settings: Vec<GroupSettingMap>,
) -> Result<EncodedDatabase, AppError> {
  let timestamp = collab_database::database::timestamp();
  let context = DatabaseContext::new(Arc::new(NoPersistenceDatabaseCollabService));
  let field_settings = default_field_settings_for_fields(&fields, database_layout);
  let mut layout_settings = LayoutSettings::default();
  if let Some(layout_setting) = layout_setting {
    layout_settings.insert(database_layout, layout_setting);
  }
  let params = CreateDatabaseParams {
    database_id: database_id.to_string(),
    fields,
    rows,
    views: vec![CreateViewParams {
      database_id: database_id.to_string(),
      view_id: view_id.to_string(),
      name: name.to_string(),
      layout: database_layout,
      layout_settings,
      filters: vec![],
      group_settings,
      sorts: vec![],
      field_settings,
      created_at: timestamp,
      modified_at: timestamp,
      ..Default::default()
    }],
  };
  let database = Database::create_with_view(params, context)
    .await
    .map_err(|err| AppError::Internal(anyhow!("Failed to create database with view: {}", err)))?;
  database
    .encode_database_collabs()
    .await
    .map_err(|err| AppError::Internal(anyhow!("Failed to encode database: {}", err)))
}

async fn prepare_default_calendar_encoded_database(
  view_id: &str,
  database_id: &str,
  name: &str,
) -> Result<EncodedDatabase, AppError> {
  let text_field = Field::from_field_type("Title", FieldType::RichText, true);
  let date_field = Field::from_field_type("Date", FieldType::DateTime, false);
  let date_field_id = date_field.id.clone();
  let multi_select_field = Field::from_field_type("Tags", FieldType::MultiSelect, false);
  let fields = vec![text_field, date_field, multi_select_field];
  let layout_setting = CalendarLayoutSetting::new(date_field_id);

  prepare_new_encoded_database(
    view_id,
    database_id,
    name,
    fields,
    vec![],
    DatabaseLayout::Calendar,
    Some(layout_setting.into()),
    vec![],
  )
  .await
}

async fn prepare_default_grid_encoded_database(
  view_id: &str,
  database_id: &str,
  name: &str,
) -> Result<EncodedDatabase, AppError> {
  let text_field = Field::from_field_type("Name", FieldType::RichText, true);
  let single_select_field = Field::from_field_type("Type", FieldType::SingleSelect, false);
  let checkbox_field = Field::from_field_type("Done", FieldType::Checkbox, false);
  let fields = vec![text_field, single_select_field, checkbox_field];
  let rows = (0..3)
    .map(|_| CreateRowParams::new(gen_row_id(), database_id.to_string()))
    .collect();

  prepare_new_encoded_database(
    view_id,
    database_id,
    name,
    fields,
    rows,
    DatabaseLayout::Grid,
    None,
    vec![],
  )
  .await
}

async fn prepare_default_board_encoded_database(
  view_id: &str,
  database_id: &str,
  name: &str,
) -> Result<EncodedDatabase, AppError> {
  let card_title_field = Field::from_field_type("Description", FieldType::RichText, true);
  let text_field_id = card_title_field.id.clone();

  let to_do_option = SelectOption::with_color("To Do", SelectOptionColor::Purple);
  let doing_option = SelectOption::with_color("Doing", SelectOptionColor::Orange);
  let done_option = SelectOption::with_color("Done", SelectOptionColor::Yellow);
  let default_option_id = to_do_option.id.clone();
  let options = vec![to_do_option, doing_option, done_option];
  let card_status_option_ids: Vec<String> =
    options.iter().map(|option| option.id.clone()).collect();
  let mut card_status_options = SingleSelectTypeOption::default();
  card_status_options.options.extend(options);
  let mut card_status_field = Field::new(
    gen_field_id(),
    "Status".to_string(),
    FieldType::SingleSelect.into(),
    false,
  );
  card_status_field.type_options.insert(
    FieldType::SingleSelect.to_string(),
    card_status_options.into(),
  );

  let card_status_field_id = card_status_field.id.clone();
  let card_status_field_type = card_status_field.field_type;
  let mut group_ids = vec![card_status_field_id.clone()];
  group_ids.extend(card_status_option_ids);
  let groups = group_ids.iter().map(|id| Group::new(id.clone())).collect();
  let group_settings: Vec<GroupSettingMap> = vec![GroupSetting {
    id: gen_database_group_id(),
    field_id: card_status_field_id.clone(),
    field_type: card_status_field_type,
    groups,
    content: Default::default(),
  }
  .into()];

  let mut rows = vec![];
  let card_status_select_option_ids = SelectOptionIds::from(vec![default_option_id.clone()]);
  for i in 0..3 {
    let card_status_cell_data = card_status_select_option_ids.to_cell(FieldType::SingleSelect);
    let mut description_cell = new_cell_builder(FieldType::RichText);
    let description_text = format!("Card {}", i + 1);
    description_cell.insert(CELL_DATA.into(), description_text.into());
    let mut row = CreateRowParams::new(gen_row_id(), database_id.to_string());
    row
      .cells
      .insert(card_status_field_id.clone(), card_status_cell_data);
    row.cells.insert(text_field_id.clone(), description_cell);
    rows.push(row);
  }
  let fields = vec![card_title_field, card_status_field];
  let layout_setting = BoardLayoutSetting::new();

  prepare_new_encoded_database(
    view_id,
    database_id,
    name,
    fields,
    rows,
    DatabaseLayout::Board,
    Some(layout_setting.into()),
    group_settings,
  )
  .await
}

#[allow(clippy::too_many_arguments)]
async fn add_new_space_to_folder(
  uid: i64,
  workspace_id: &str,
  view_id: &str,
  folder: &mut Folder,
  space_permission: &SpacePermission,
  name: &str,
  space_icon: &str,
  space_icon_color: &str,
) -> Result<FolderUpdate, AppError> {
  let encoded_update = {
    let view = NestedChildViewBuilder::new(uid, workspace_id.to_string())
      .with_view_id(view_id)
      .with_name(name)
      .with_extra(|builder| {
        let mut extra = builder
          .is_space(true, to_space_permission(space_permission))
          .build();
        extra["space_icon_color"] = json!(space_icon_color);
        extra["space_icon"] = json!(space_icon);
        extra
      })
      .build()
      .view;
    let mut txn = folder.collab.transact_mut();
    folder.body.views.insert(&mut txn, view, None);
    if *space_permission == SpacePermission::Private {
      folder
        .body
        .views
        .update_view(&mut txn, view_id, |update| update.set_private(true).done());
    }
    txn.encode_update_v1()
  };
  Ok(FolderUpdate {
    updated_encoded_collab: folder_to_encoded_collab(folder)?,
    encoded_update,
  })
}

async fn update_space_properties(
  view_id: &str,
  folder: &mut Folder,
  space_permission: &SpacePermission,
  name: &str,
  space_icon: &str,
  space_icon_color: &str,
) -> Result<FolderUpdate, AppError> {
  let encoded_update = {
    let mut txn = folder.collab.transact_mut();
    folder.body.views.update_view(&mut txn, view_id, |update| {
      let extra = json!({
        "is_space": true,
        "space_permission": to_space_permission(space_permission) as u8,
        "space_created_at": timestamp(),
        "space_icon": space_icon,
        "space_icon_color": space_icon_color,
      })
      .to_string();
      let is_private = *space_permission == SpacePermission::Private;
      update
        .set_name(name)
        .set_extra(&extra)
        .set_private(is_private)
        .done()
    });
    txn.encode_update_v1()
  };
  Ok(FolderUpdate {
    updated_encoded_collab: folder_to_encoded_collab(folder)?,
    encoded_update,
  })
}

async fn add_new_database_to_workspace(
  workspace_database: &mut WorkspaceDatabase,
  database_id: &str,
  view_id: &str,
) -> Result<WorkspaceDatabaseUpdate, AppError> {
  let view_ids_by_database_id =
    HashMap::from([(database_id.to_string(), vec![view_id.to_string()])]);
  let encoded_updates = workspace_database
    .batch_add_database(view_ids_by_database_id)
    .encode_update_v1();
  let updated_encoded_collab = workspace_database_to_encoded_collab(workspace_database)?;
  Ok(WorkspaceDatabaseUpdate {
    updated_encoded_collab,
    encoded_updates,
  })
}

async fn add_new_view_to_folder(
  uid: i64,
  parent_view_id: &str,
  view_id: &str,
  folder: &mut Folder,
  name: Option<&str>,
  layout: collab_folder::ViewLayout,
) -> Result<FolderUpdate, AppError> {
  let encoded_update = {
    let view = NestedChildViewBuilder::new(uid, parent_view_id.to_string())
      .with_view_id(view_id)
      .with_name(name.unwrap_or_default())
      .with_layout(layout)
      .build()
      .view;
    let mut txn = folder.collab.transact_mut();
    folder.body.views.insert(&mut txn, view, None);

    txn.encode_update_v1()
  };
  Ok(FolderUpdate {
    updated_encoded_collab: folder_to_encoded_collab(folder)?,
    encoded_update,
  })
}

async fn update_view_properties(
  view_id: &str,
  folder: &mut Folder,
  name: &str,
  icon: Option<&ViewIcon>,
  extra: Option<impl AsRef<str>>,
) -> Result<FolderUpdate, AppError> {
  let encoded_update = {
    let mut txn = folder.collab.transact_mut();
    let icon = icon.map(|icon| to_folder_view_icon(icon.clone()));
    folder.body.views.update_view(&mut txn, view_id, |update| {
      update
        .set_name(name)
        .set_icon(icon)
        .set_extra_if_not_none(extra)
        .done()
    });
    txn.encode_update_v1()
  };
  Ok(FolderUpdate {
    updated_encoded_collab: folder_to_encoded_collab(folder)?,
    encoded_update,
  })
}

async fn move_view(
  view_id: &str,
  new_parent_view_id: &str,
  prev_view_id: Option<String>,
  folder: &mut Folder,
) -> Result<FolderUpdate, AppError> {
  let encoded_update = {
    let mut txn = folder.collab.transact_mut();
    folder
      .body
      .move_nested_view(&mut txn, view_id, new_parent_view_id, prev_view_id);
    txn.encode_update_v1()
  };
  Ok(FolderUpdate {
    updated_encoded_collab: folder_to_encoded_collab(folder)?,
    encoded_update,
  })
}

async fn move_view_to_trash(view_id: &str, folder: &mut Folder) -> Result<FolderUpdate, AppError> {
  let mut current_view_and_descendants = folder
    .get_views_belong_to(view_id)
    .iter()
    .map(|v| v.id.clone())
    .collect_vec();
  current_view_and_descendants.push(view_id.to_string());

  let encoded_update = {
    let mut txn = folder.collab.transact_mut();
    current_view_and_descendants.iter().for_each(|view_id| {
      folder.body.views.update_view(&mut txn, view_id, |update| {
        update.set_favorite(false).done()
      });
    });
    folder
      .body
      .views
      .update_view(&mut txn, view_id, |update| update.set_trash(true).done());
    txn.encode_update_v1()
  };

  Ok(FolderUpdate {
    updated_encoded_collab: folder_to_encoded_collab(folder)?,
    encoded_update,
  })
}

async fn move_view_out_from_trash(
  view_id: &str,
  folder: &mut Folder,
) -> Result<FolderUpdate, AppError> {
  let encoded_update = {
    let mut txn = folder.collab.transact_mut();
    folder
      .body
      .views
      .update_view(&mut txn, view_id, |update| update.set_trash(false).done());
    txn.encode_update_v1()
  };

  Ok(FolderUpdate {
    updated_encoded_collab: folder_to_encoded_collab(folder)?,
    encoded_update,
  })
}

async fn move_all_views_out_from_trash(folder: &mut Folder) -> Result<FolderUpdate, AppError> {
  let encoded_update = {
    let mut txn = folder.collab.transact_mut();
    if let Some(op) = folder
      .body
      .section
      .section_op(&txn, collab_folder::Section::Trash)
    {
      op.clear(&mut txn);
    };
    txn.encode_update_v1()
  };

  Ok(FolderUpdate {
    updated_encoded_collab: folder_to_encoded_collab(folder)?,
    encoded_update,
  })
}

fn folder_to_encoded_collab(folder: &Folder) -> Result<Vec<u8>, AppError> {
  let collab_type = CollabType::Folder;
  let encoded_folder_collab = folder
    .encode_collab_v1(|collab| collab_type.validate_require_data(collab))
    .map_err(|err| AppError::Internal(anyhow!("Failed to encode workspace folder: {}", err)))?;
  encoded_folder_collab.encode_to_bytes().map_err(|err| {
    AppError::Internal(anyhow!(
      "Failed to encode workspace folder to bytes: {}",
      err
    ))
  })
}

fn workspace_database_to_encoded_collab(
  workspace_db: &WorkspaceDatabase,
) -> Result<Vec<u8>, AppError> {
  let encoded_workspace_db_collab = workspace_db
    .encode_collab_v1()
    .map_err(|err| AppError::Internal(anyhow!("Failed to encode workspace folder: {}", err)))?;
  encoded_workspace_db_collab
    .encode_to_bytes()
    .map_err(|err| {
      AppError::Internal(anyhow!(
        "Failed to encode workspace folder to bytes: {}",
        err
      ))
    })
}

async fn insert_and_broadcast_workspace_database_update(
  uid: i64,
  workspace_id: Uuid,
  workspace_database_id: &str,
  workspace_database_update: WorkspaceDatabaseUpdate,
  collab_storage: &CollabAccessControlStorage,
  transaction: &mut Transaction<'_, sqlx::Postgres>,
) -> Result<(), AppError> {
  let params = CollabParams {
    object_id: workspace_database_id.to_string(),
    encoded_collab_v1: workspace_database_update.updated_encoded_collab.into(),
    collab_type: CollabType::WorkspaceDatabase,
  };
  let action_description = format!("Update workspace database: {}", workspace_id);
  collab_storage
    .upsert_new_collab_with_transaction(
      &workspace_id.to_string(),
      &uid,
      params,
      transaction,
      &action_description,
    )
    .await?;
  broadcast_update(
    collab_storage,
    workspace_database_id,
    workspace_database_update.encoded_updates.clone(),
  )
  .await?;
  Ok(())
}

async fn insert_and_broadcast_workspace_folder_update(
  uid: i64,
  workspace_id: Uuid,
  folder_update: FolderUpdate,
  collab_storage: &CollabAccessControlStorage,
  transaction: &mut Transaction<'_, sqlx::Postgres>,
) -> Result<(), AppError> {
  let params = CollabParams {
    object_id: workspace_id.to_string(),
    encoded_collab_v1: folder_update.updated_encoded_collab.into(),
    collab_type: CollabType::Folder,
  };
  let action_description = format!("Update workspace folder: {}", workspace_id);
  collab_storage
    .upsert_new_collab_with_transaction(
      &workspace_id.to_string(),
      &uid,
      params,
      transaction,
      &action_description,
    )
    .await?;
  broadcast_update(
    collab_storage,
    &workspace_id.to_string(),
    folder_update.encoded_update.clone(),
  )
  .await?;
  Ok(())
}

async fn create_document_page(
  pg_pool: &PgPool,
  collab_storage: &CollabAccessControlStorage,
  uid: i64,
  workspace_id: Uuid,
  parent_view_id: &str,
  name: Option<&str>,
) -> Result<Page, AppError> {
  let default_document_collab_params = prepare_default_document_collab_param()?;
  let view_id = default_document_collab_params.object_id.clone();
  let collab_origin = GetCollabOrigin::User { uid };
  let mut folder =
    get_latest_collab_folder(collab_storage, collab_origin, &workspace_id.to_string()).await?;
  let folder_update = add_new_view_to_folder(
    uid,
    parent_view_id,
    &view_id,
    &mut folder,
    name,
    collab_folder::ViewLayout::Document,
  )
  .await?;
  let mut transaction = pg_pool.begin().await?;
  let start = Instant::now();
  let action = format!("Create new collab: {}", view_id);
  collab_storage
    .upsert_new_collab_with_transaction(
      &workspace_id.to_string(),
      &uid,
      default_document_collab_params,
      &mut transaction,
      &action,
    )
    .await?;
  insert_and_broadcast_workspace_folder_update(
    uid,
    workspace_id,
    folder_update,
    collab_storage,
    &mut transaction,
  )
  .await?;
  transaction.commit().await?;
  collab_storage.metrics().observe_pg_tx(start.elapsed());
  Ok(Page { view_id })
}

async fn create_grid_page(
  pg_pool: &PgPool,
  collab_storage: &CollabAccessControlStorage,
  uid: i64,
  workspace_id: Uuid,
  parent_view_id: &str,
  name: Option<&str>,
) -> Result<Page, AppError> {
  let view_id = Uuid::new_v4().to_string();
  let database_id = gen_database_id();
  let default_grid_encoded_database =
    prepare_default_grid_encoded_database(&view_id, &database_id, name.unwrap_or_default()).await?;
  create_database_page(
    pg_pool,
    collab_storage,
    uid,
    workspace_id,
    parent_view_id,
    &view_id,
    collab_folder::ViewLayout::Grid,
    name,
    &default_grid_encoded_database,
  )
  .await
}

async fn create_board_page(
  pg_pool: &PgPool,
  collab_storage: &CollabAccessControlStorage,
  uid: i64,
  workspace_id: Uuid,
  parent_view_id: &str,
  name: Option<&str>,
) -> Result<Page, AppError> {
  let view_id = Uuid::new_v4().to_string();
  let database_id = gen_database_id();
  let default_board_encoded_database =
    prepare_default_board_encoded_database(&view_id, &database_id, name.unwrap_or_default())
      .await?;
  create_database_page(
    pg_pool,
    collab_storage,
    uid,
    workspace_id,
    parent_view_id,
    &view_id,
    collab_folder::ViewLayout::Board,
    name,
    &default_board_encoded_database,
  )
  .await
}

async fn create_calendar_page(
  pg_pool: &PgPool,
  collab_storage: &CollabAccessControlStorage,
  uid: i64,
  workspace_id: Uuid,
  parent_view_id: &str,
  name: Option<&str>,
) -> Result<Page, AppError> {
  let view_id = Uuid::new_v4().to_string();
  let database_id = gen_database_id();
  let default_calendar_encoded_database =
    prepare_default_calendar_encoded_database(&view_id, &database_id, name.unwrap_or_default())
      .await?;
  create_database_page(
    pg_pool,
    collab_storage,
    uid,
    workspace_id,
    parent_view_id,
    &view_id,
    collab_folder::ViewLayout::Calendar,
    name,
    &default_calendar_encoded_database,
  )
  .await
}

#[allow(clippy::too_many_arguments)]
async fn create_database_page(
  pg_pool: &PgPool,
  collab_storage: &CollabAccessControlStorage,
  uid: i64,
  workspace_id: Uuid,
  parent_view_id: &str,
  view_id: &str,
  view_layout: collab_folder::ViewLayout,
  name: Option<&str>,
  encoded_database: &EncodedDatabase,
) -> Result<Page, AppError> {
  let collab_origin = GetCollabOrigin::User { uid };
  let mut folder = get_latest_collab_folder(
    collab_storage,
    collab_origin.clone(),
    &workspace_id.to_string(),
  )
  .await?;
  let folder_update =
    add_new_view_to_folder(uid, parent_view_id, view_id, &mut folder, name, view_layout).await?;
  let (workspace_database_id, mut workspace_database) =
    get_latest_workspace_database(collab_storage, pg_pool, collab_origin, workspace_id).await?;
  let database_id = encoded_database.encoded_database_collab.object_id.clone();
  let workspace_database_update =
    add_new_database_to_workspace(&mut workspace_database, &database_id, view_id).await?;
  let database_collab_params = CollabParams {
    object_id: database_id.clone(),
    encoded_collab_v1: encoded_database
      .encoded_database_collab
      .encoded_collab
      .encode_to_bytes()?
      .into(),
    collab_type: CollabType::Database,
  };
  let row_collab_params_list = encoded_database
    .encoded_row_collabs
    .iter()
    .map(|row_collab| CollabParams {
      object_id: row_collab.object_id.clone(),
      encoded_collab_v1: row_collab.encoded_collab.encode_to_bytes().unwrap().into(),
      collab_type: CollabType::DatabaseRow,
    })
    .collect_vec();

  let mut transaction = pg_pool.begin().await?;
  let start = Instant::now();
  let action = format!("Create new database collab: {}", database_id);
  collab_storage
    .upsert_new_collab_with_transaction(
      &workspace_id.to_string(),
      &uid,
      database_collab_params,
      &mut transaction,
      &action,
    )
    .await?;
  collab_storage
    .batch_insert_new_collab(&workspace_id.to_string(), &uid, row_collab_params_list)
    .await?;
  insert_and_broadcast_workspace_folder_update(
    uid,
    workspace_id,
    folder_update,
    collab_storage,
    &mut transaction,
  )
  .await?;
  insert_and_broadcast_workspace_database_update(
    uid,
    workspace_id,
    &workspace_database_id,
    workspace_database_update,
    collab_storage,
    &mut transaction,
  )
  .await?;
  transaction.commit().await?;
  collab_storage.metrics().observe_pg_tx(start.elapsed());
  Ok(Page {
    view_id: view_id.to_string(),
  })
}

pub async fn move_page(
  pg_pool: &PgPool,
  collab_storage: &CollabAccessControlStorage,
  uid: i64,
  workspace_id: Uuid,
  view_id: &str,
  new_parent_view_id: &str,
  prev_view_id: Option<String>,
) -> Result<(), AppError> {
  let collab_origin = GetCollabOrigin::User { uid };
  let mut folder =
    get_latest_collab_folder(collab_storage, collab_origin, &workspace_id.to_string()).await?;
  let folder_update = move_view(view_id, new_parent_view_id, prev_view_id, &mut folder).await?;
  let mut transaction = pg_pool.begin().await?;
  insert_and_broadcast_workspace_folder_update(
    uid,
    workspace_id,
    folder_update,
    collab_storage,
    &mut transaction,
  )
  .await?;
  transaction.commit().await?;
  Ok(())
}

pub async fn move_page_to_trash(
  pg_pool: &PgPool,
  collab_storage: &CollabAccessControlStorage,
  uid: i64,
  workspace_id: Uuid,
  view_id: &str,
) -> Result<(), AppError> {
  let collab_origin = GetCollabOrigin::User { uid };
  let mut folder =
    get_latest_collab_folder(collab_storage, collab_origin, &workspace_id.to_string()).await?;
  let trash_info = folder.get_my_trash_info();
  if trash_info.into_iter().any(|info| info.id == view_id) {
    return Ok(());
  }
  let folder_update = move_view_to_trash(view_id, &mut folder).await?;
  let mut transaction = pg_pool.begin().await?;
  insert_and_broadcast_workspace_folder_update(
    uid,
    workspace_id,
    folder_update,
    collab_storage,
    &mut transaction,
  )
  .await?;
  transaction.commit().await?;
  Ok(())
}

pub async fn restore_page_from_trash(
  pg_pool: &PgPool,
  collab_storage: &CollabAccessControlStorage,
  uid: i64,
  workspace_id: Uuid,
  view_id: &str,
) -> Result<(), AppError> {
  let collab_origin = GetCollabOrigin::User { uid };
  let mut folder =
    get_latest_collab_folder(collab_storage, collab_origin, &workspace_id.to_string()).await?;
  let folder_update = move_view_out_from_trash(view_id, &mut folder).await?;
  let mut transaction = pg_pool.begin().await?;
  insert_and_broadcast_workspace_folder_update(
    uid,
    workspace_id,
    folder_update,
    collab_storage,
    &mut transaction,
  )
  .await?;
  transaction.commit().await?;
  Ok(())
}

pub async fn restore_all_pages_from_trash(
  pg_pool: &PgPool,
  collab_storage: &CollabAccessControlStorage,
  uid: i64,
  workspace_id: Uuid,
) -> Result<(), AppError> {
  let collab_origin = GetCollabOrigin::User { uid };
  let mut folder =
    get_latest_collab_folder(collab_storage, collab_origin, &workspace_id.to_string()).await?;
  let folder_update = move_all_views_out_from_trash(&mut folder).await?;
  let mut transaction = pg_pool.begin().await?;
  insert_and_broadcast_workspace_folder_update(
    uid,
    workspace_id,
    folder_update,
    collab_storage,
    &mut transaction,
  )
  .await?;
  transaction.commit().await?;
  Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn update_page(
  pg_pool: &PgPool,
  collab_storage: &CollabAccessControlStorage,
  uid: i64,
  workspace_id: Uuid,
  view_id: &str,
  name: &str,
  icon: Option<&ViewIcon>,
  extra: Option<impl AsRef<str>>,
) -> Result<(), AppError> {
  let collab_origin = GetCollabOrigin::User { uid };
  let mut folder =
    get_latest_collab_folder(collab_storage, collab_origin, &workspace_id.to_string()).await?;
  let folder_update = update_view_properties(view_id, &mut folder, name, icon, extra).await?;
  let mut transaction = pg_pool.begin().await?;
  insert_and_broadcast_workspace_folder_update(
    uid,
    workspace_id,
    folder_update,
    collab_storage,
    &mut transaction,
  )
  .await?;
  transaction.commit().await?;

  Ok(())
}

pub async fn get_page_view_collab(
  pg_pool: &PgPool,
  collab_access_control_storage: &CollabAccessControlStorage,
  uid: i64,
  workspace_id: Uuid,
  view_id: &str,
) -> Result<PageCollab, AppError> {
  let folder = get_latest_collab_folder(
    collab_access_control_storage,
    GetCollabOrigin::User { uid },
    &workspace_id.to_string(),
  )
  .await?;
  let view = folder
    .get_view(view_id)
    .ok_or(AppError::InvalidFolderView(format!(
      "View {} not found",
      view_id
    )))?;

  let owner = match view.created_by {
    Some(uid) => Some(select_web_user_from_uid(pg_pool, uid).await?),
    None => None,
  };
  let last_editor = match view.last_edited_by {
    Some(uid) => Some(select_web_user_from_uid(pg_pool, uid).await?),
    None => None,
  };
  let publish_view_ids = select_published_view_ids_for_workspace(pg_pool, workspace_id).await?;
  let publish_view_ids: HashSet<String> = publish_view_ids
    .into_iter()
    .map(|id| id.to_string())
    .collect();
  let folder_view = FolderView {
    view_id: view_id.to_string(),
    name: view.name.clone(),
    icon: view
      .icon
      .as_ref()
      .map(|icon| to_dto_view_icon(icon.clone())),
    is_space: check_if_view_is_space(&view),
    is_private: false,
    is_published: publish_view_ids.contains(view_id),
    layout: to_dto_view_layout(&view.layout),
    created_at: DateTime::from_timestamp(view.created_at, 0).unwrap_or_default(),
    last_edited_time: DateTime::from_timestamp(view.last_edited_time, 0).unwrap_or_default(),
    extra: view.extra.as_ref().map(|e| parse_extra_field_as_json(e)),
    children: vec![],
  };
  let page_collab_data = match view.layout {
    collab_folder::ViewLayout::Document => {
      get_page_collab_data_for_document(collab_access_control_storage, uid, workspace_id, view_id)
        .await
    },
    collab_folder::ViewLayout::Grid
    | collab_folder::ViewLayout::Board
    | collab_folder::ViewLayout::Calendar => {
      get_page_collab_data_for_database(
        pg_pool,
        collab_access_control_storage,
        uid,
        workspace_id,
        view_id,
      )
      .await
    },
    collab_folder::ViewLayout::Chat => Err(AppError::InvalidRequest(
      "Page view for AI chat is not supported at the moment".to_string(),
    )),
  }?;

  let page_collab = PageCollab {
    view: folder_view,
    data: page_collab_data,
    owner,
    last_editor,
  };

  Ok(page_collab)
}

async fn get_page_collab_data_for_database(
  pg_pool: &PgPool,
  collab_access_control_storage: &CollabAccessControlStorage,
  uid: i64,
  workspace_id: Uuid,
  view_id: &str,
) -> Result<PageCollabData, AppError> {
  let ws_db_oid = select_workspace_database_oid(pg_pool, &workspace_id).await?;
  let ws_db = get_latest_collab_encoded(
    collab_access_control_storage,
    GetCollabOrigin::User { uid },
    &workspace_id.to_string(),
    &ws_db_oid,
    CollabType::WorkspaceDatabase,
  )
  .await?;
  let ws_db_collab = collab_from_doc_state(ws_db.doc_state.to_vec(), &ws_db_oid)?;
  let ws_db_body = WorkspaceDatabase::open(ws_db_collab).map_err(|err| {
    AppError::Internal(anyhow!("Failed to open workspace database body: {}", err))
  })?;
  let db_oid = {
    ws_db_body
      .get_database_meta_with_view_id(view_id)
      .ok_or(AppError::NoRequiredData(format!(
        "Database view {} not found",
        view_id
      )))?
      .database_id
  };
  let db = get_latest_collab_encoded(
    collab_access_control_storage,
    GetCollabOrigin::User { uid },
    &workspace_id.to_string(),
    &db_oid,
    CollabType::Database,
  )
  .await?;
  let db_collab = Collab::new_with_source(
    CollabOrigin::Server,
    &db_oid,
    db.clone().into(),
    vec![],
    false,
  )
  .map_err(|err| {
    AppError::Internal(anyhow!(
      "Unable to create collab from object id {}: {}",
      &db_oid,
      err
    ))
  })?;
  let db_body = DatabaseBody::from_collab(
    &db_collab,
    Arc::new(NoPersistenceDatabaseCollabService),
    None,
  )
  .ok_or_else(|| AppError::RecordNotFound("no database body found".to_string()))?;
  let inline_view_id = {
    let txn = db_collab.transact();
    db_body.get_inline_view_id(&txn)
  };
  let row_ids: Vec<RowId> = {
    let txn = db_collab.transact();
    db_body
      .views
      .get_row_orders(&txn, &inline_view_id)
      .iter()
      .map(|ro| ro.id.clone())
      .collect()
  };
  let queries: Vec<QueryCollab> = row_ids
    .iter()
    .map(|row_id| QueryCollab {
      object_id: row_id.to_string(),
      collab_type: CollabType::DatabaseRow,
    })
    .collect();
  let row_query_collab_results = collab_access_control_storage
    .batch_get_collab(&uid, &workspace_id.to_string(), queries, true)
    .await;
  let row_data = tokio::task::spawn_blocking(move || {
    let row_collabs: HashMap<String, Vec<u8>> = row_query_collab_results
      .into_par_iter()
      .filter_map(|(row_id, query_collab_result)| match query_collab_result {
        QueryCollabResult::Success { encode_collab_v1 } => {
          let decoded_result = EncodedCollab::decode_from_bytes(&encode_collab_v1);
          match decoded_result {
            Ok(decoded) => Some((row_id, decoded.doc_state.to_vec())),
            Err(err) => {
              tracing::error!("Failed to decode collab for row {}: {}", row_id, err);
              None
            },
          }
        },
        QueryCollabResult::Failed { error } => {
          tracing::error!("Failed to get collab: {:?}", error);
          None
        },
      })
      .collect();
    row_collabs
  })
  .await?;

  Ok(PageCollabData {
    encoded_collab: db.doc_state.to_vec(),
    row_data,
  })
}

async fn get_page_collab_data_for_document(
  collab_access_control_storage: &CollabAccessControlStorage,
  uid: i64,
  workspace_id: Uuid,
  view_id: &str,
) -> Result<PageCollabData, AppError> {
  let collab = get_latest_collab_encoded(
    collab_access_control_storage,
    GetCollabOrigin::User { uid },
    &workspace_id.to_string(),
    view_id,
    CollabType::Document,
  )
  .await?;
  Ok(PageCollabData {
    encoded_collab: collab.doc_state.clone().to_vec(),
    row_data: HashMap::default(),
  })
}

#[instrument(level = "debug", skip_all)]
pub async fn update_page_collab_data(
  appflowy_web_metrics: &Arc<AppFlowyWebMetrics>,
  server: Data<RealtimeServerAddr>,
  user: RealtimeUser,
  workspace_id: Uuid,
  object_id: Uuid,
  collab_type: CollabType,
  doc_state: Vec<u8>,
) -> Result<(), AppError> {
  let object_id = object_id.to_string();
  appflowy_web_metrics.record_update_size_bytes(doc_state.len());

  let message = ClientHttpUpdateMessage {
    user,
    workspace_id: workspace_id.to_string(),
    object_id: object_id.to_string(),
    collab_type,
    update: Bytes::from(doc_state),
    state_vector: None,
    return_tx: None,
  };

  server
    .try_send(message)
    .map_err(|err| AppError::Internal(anyhow!("Failed to send message to server: {}", err)))?;

  Ok(())
}
