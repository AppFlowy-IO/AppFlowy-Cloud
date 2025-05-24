use crate::api::metrics::AppFlowyWebMetrics;
use crate::api::ws::RealtimeServerAddr;
use crate::biz::chat::ops::create_chat;
use crate::biz::collab::database::{
  resolve_dependencies_when_create_database_linked_view, LinkedViewDependencies,
};
use crate::biz::collab::folder_view::{
  check_if_view_is_space, get_prev_view_id, parse_extra_field_as_json, to_dto_view_icon,
  to_dto_view_layout, to_folder_view_icon, to_folder_view_layout, to_space_permission,
};
use crate::biz::collab::ops::get_latest_workspace_database;
use crate::biz::collab::utils::{
  batch_get_latest_collab_encoded, collab_from_doc_state, collab_to_doc_state,
  get_latest_collab_database_body, get_latest_collab_encoded, get_latest_collab_folder,
};
use actix_web::web::Data;
use anyhow::anyhow;
use app_error::AppError;
use appflowy_collaborate::actix_ws::entities::ClientHttpUpdateMessage;
use appflowy_collaborate::collab::storage::CollabAccessControlStorage;
use bytes::Bytes;
use chrono::DateTime;
use collab::core::collab::{default_client_id, Collab, CollabOptions};
use collab_database::database::{
  gen_database_group_id, gen_database_id, gen_field_id, gen_row_id, Database, DatabaseContext,
};
use collab_database::entity::{CreateDatabaseParams, CreateViewParams, EncodedDatabase, FieldType};
use collab_database::fields::select_type_option::{
  SelectOption, SelectOptionColor, SelectOptionIds, SingleSelectTypeOption,
};
use collab_database::fields::{
  default_field_settings_by_layout_map, default_field_settings_for_fields, Field,
};
use collab_database::rows::{new_cell_builder, CreateRowParams};
use collab_database::template::entity::CELL_DATA;
use collab_database::views::{
  BoardLayoutSetting, CalendarLayoutSetting, DatabaseLayout, Group, GroupSetting, GroupSettingMap,
  LayoutSetting, LayoutSettings,
};
use collab_database::workspace_database::{NoPersistenceDatabaseCollabService, WorkspaceDatabase};
use collab_database::{database::DatabaseBody, rows::RowId};
use collab_document::document::{Document, DocumentBody};
use collab_document::document_data::default_document_data;
use collab_entity::{CollabType, EncodedCollab};
use collab_folder::hierarchy_builder::NestedChildViewBuilder;
use collab_folder::{timestamp, CollabOrigin, Folder, SectionItem, SpaceInfo};
use collab_rt_entity::user::RealtimeUser;
use database::collab::{select_workspace_database_oid, CollabStorage, GetCollabOrigin};
use database::publish::select_published_view_ids_for_workspace;
use database::user::select_web_user_from_uid;
use database_entity::dto::{
  CollabParams, PublishCollabItem, PublishCollabMetadata, QueryCollab, QueryCollabResult,
};
use fancy_regex::Regex;
use itertools::Itertools;
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use serde_json::json;
use shared_entity::dto::chat_dto::CreateChatParams;
use shared_entity::dto::publish_dto::{PublishDatabaseData, PublishViewInfo, PublishViewMetaData};
use shared_entity::dto::workspace_dto::{
  FolderView, Page, PageCollab, PageCollabData, Space, SpacePermission, ViewIcon, ViewLayout,
};
use sqlx::PgPool;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant};
use tokio::time::timeout_at;
use tracing::instrument;
use uuid::Uuid;
use workspace_template::document::parser::{JsonToDocumentParser, SerdeBlock};
use yrs::block::ClientID;

use super::publish::PublishedCollabStore;

#[allow(clippy::too_many_arguments)]
pub async fn update_space(
  appflowy_web_metrics: &AppFlowyWebMetrics,
  server: Data<RealtimeServerAddr>,
  user: RealtimeUser,
  collab_storage: &CollabAccessControlStorage,
  workspace_id: Uuid,
  view_id: &str,
  space_permission: &SpacePermission,
  name: &str,
  space_icon: &str,
  space_icon_color: &str,
) -> Result<(), AppError> {
  let collab_origin = GetCollabOrigin::User { uid: user.uid };
  let mut folder = get_latest_collab_folder(collab_storage, collab_origin, workspace_id, default_client_id()).await?;
  let folder_update = update_space_properties(
    view_id,
    &mut folder,
    space_permission,
    name,
    space_icon,
    space_icon_color,
  )
  .await?;
  update_workspace_folder_data(
    appflowy_web_metrics,
    server,
    user,
    workspace_id,
    folder_update,
  )
  .await?;
  Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn create_space(
  appflowy_web_metrics: &AppFlowyWebMetrics,
  server: Data<RealtimeServerAddr>,
  user: RealtimeUser,
  pg_pool: &PgPool,
  collab_storage: &CollabAccessControlStorage,
  workspace_id: Uuid,
  space_permission: &SpacePermission,
  name: &str,
  space_icon: &str,
  space_color: &str,
  view_id_override: Option<Uuid>,
) -> Result<Space, AppError> {
  let view_id = view_id_override.unwrap_or(Uuid::new_v4());
  let client_id = default_client_id();
  let default_document_collab_params = prepare_default_document_collab_param(client_id, view_id)?;
  let collab_origin = GetCollabOrigin::User { uid: user.uid };
  let mut folder = get_latest_collab_folder(collab_storage, collab_origin, workspace_id, client_id).await?;
  let folder_update = add_new_space_to_folder(
    user.uid,
    &workspace_id,
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
      workspace_id,
      &user.uid,
      default_document_collab_params,
      &mut transaction,
      &action,
    )
    .await?;
  update_workspace_folder_data(
    appflowy_web_metrics,
    server,
    user,
    workspace_id,
    folder_update,
  )
  .await?;
  transaction.commit().await?;
  collab_storage.metrics().observe_pg_tx(start.elapsed());
  Ok(Space { view_id })
}

// Different from create page as this function does not create an associated collab
#[allow(clippy::too_many_arguments)]
pub async fn create_folder_view(
  appflowy_web_metrics: &AppFlowyWebMetrics,
  server: Data<RealtimeServerAddr>,
  user: RealtimeUser,
  collab_storage: &CollabAccessControlStorage,
  pg_pool: &PgPool,
  workspace_id: Uuid,
  parent_view_id: &Uuid,
  view_layout: ViewLayout,
  name: Option<&str>,
  view_id: Option<Uuid>,
  database_id: Option<Uuid>,
) -> Result<Page, AppError> {
  let view_id = view_id.unwrap_or_else(Uuid::new_v4);
  let collab_origin = GetCollabOrigin::User { uid: user.uid };
  let mut folder =
    get_latest_collab_folder(collab_storage, collab_origin.clone(), workspace_id, default_client_id()).await?;
  let folder_update = add_new_view_to_folder(
    user.uid,
    parent_view_id,
    &view_id,
    &mut folder,
    name,
    to_folder_view_layout(view_layout),
  )
  .await?;
  let (workspace_database_id, workspace_database_update) = if let Some(database_id) = database_id {
    let (workspace_database_id, mut workspace_database) =
      get_latest_workspace_database(collab_storage, pg_pool, collab_origin, workspace_id).await?;
    let workspace_database_update = add_new_database_view_for_workspace_database(
      &mut workspace_database,
      &database_id.to_string(),
      &view_id,
    )
    .await?;
    (Some(workspace_database_id), Some(workspace_database_update))
  } else {
    (None, None)
  };

  update_workspace_folder_data(
    appflowy_web_metrics,
    server.clone(),
    user.clone(),
    workspace_id,
    folder_update,
  )
  .await?;

  if let (Some(workspace_database_id), Some(workspace_database_update)) =
    (workspace_database_id, workspace_database_update)
  {
    update_workspace_database_data(
      appflowy_web_metrics,
      server,
      user,
      workspace_id,
      workspace_database_id,
      workspace_database_update,
    )
    .await?;
  }
  Ok(Page { view_id })
}

#[allow(clippy::too_many_arguments)]
pub async fn create_page(
  appflowy_web_metrics: &AppFlowyWebMetrics,
  server: Data<RealtimeServerAddr>,
  user: RealtimeUser,
  pg_pool: &PgPool,
  collab_storage: &CollabAccessControlStorage,
  workspace_id: Uuid,
  parent_view_id: &Uuid,
  view_layout: &ViewLayout,
  name: Option<&str>,
  page_data: Option<&serde_json::Value>,
  view_id: Option<Uuid>,
  collab_id: Option<Uuid>,
) -> Result<Page, AppError> {
  match view_layout {
    ViewLayout::Document => {
      create_document_page(
        appflowy_web_metrics,
        server,
        user,
        pg_pool,
        collab_storage,
        workspace_id,
        parent_view_id,
        name,
        page_data,
        view_id,
        collab_id,
      )
      .await
    },
    //TODO: allow view id and database id to be overriden
    ViewLayout::Grid => {
      create_grid_page(
        appflowy_web_metrics,
        server,
        user,
        pg_pool,
        collab_storage,
        workspace_id,
        parent_view_id,
        name,
      )
      .await
    },
    ViewLayout::Calendar => {
      create_calendar_page(
        appflowy_web_metrics,
        server,
        user,
        pg_pool,
        collab_storage,
        workspace_id,
        parent_view_id,
        name,
      )
      .await
    },
    ViewLayout::Board => {
      create_board_page(
        appflowy_web_metrics,
        server,
        user,
        pg_pool,
        collab_storage,
        workspace_id,
        parent_view_id,
        name,
      )
      .await
    },
    ViewLayout::Chat => {
      create_chat_page(
        appflowy_web_metrics,
        server,
        user,
        pg_pool,
        collab_storage,
        workspace_id,
        parent_view_id,
        name,
      )
      .await
    },
  }
}

fn prepare_document_collab_param_with_initial_data(
  client_id: ClientID,
  page_data: serde_json::Value,
  collab_id: Uuid,
) -> Result<CollabParams, AppError> {
  let options = CollabOptions::new(collab_id.to_string(), client_id);
  let collab = Collab::new_with_options(CollabOrigin::Empty, options)
    .map_err(|e| AppError::Internal(e.into()))?;
  let document_data = JsonToDocumentParser::json_to_document(page_data)?;
  let document = Document::create_with_data(collab, document_data)
    .map_err(|err| AppError::InvalidPageData(err.to_string()))?;
  let encoded_collab_v1 = document
    .encode_collab()
    .map_err(|err| {
      AppError::Internal(anyhow!(
        "Failed to encode document with initial data: {}",
        err
      ))
    })?
    .encode_to_bytes()?;
  Ok(CollabParams {
    object_id: collab_id,
    encoded_collab_v1: encoded_collab_v1.into(),
    collab_type: CollabType::Document,
    updated_at: None,
  })
}

fn prepare_default_document_collab_param(
  client_id: ClientID,
  collab_id: Uuid,
) -> Result<CollabParams, AppError> {
  let object_id = collab_id.to_string();
  let document_data = default_document_data(&object_id);
  let document = Document::create(&object_id, document_data, client_id)
    .map_err(|err| AppError::Internal(anyhow!("Failed to create default document: {}", err)))?;
  let encoded_collab_v1 = document
    .encode_collab()
    .map_err(|err| AppError::Internal(anyhow!("Failed to encode default document: {}", err)))?
    .encode_to_bytes()?;
  Ok(CollabParams {
    object_id: collab_id,
    encoded_collab_v1: encoded_collab_v1.into(),
    collab_type: CollabType::Document,
    updated_at: None,
  })
}

#[allow(clippy::too_many_arguments)]
async fn prepare_new_encoded_database(
  view_id: &Uuid,
  database_id: &Uuid,
  name: &str,
  fields: Vec<Field>,
  rows: Vec<CreateRowParams>,
  database_layout: DatabaseLayout,
  layout_setting: Option<LayoutSetting>,
  group_settings: Vec<GroupSettingMap>,
) -> Result<EncodedDatabase, AppError> {
  let timestamp = collab_database::database::timestamp();
  let context = DatabaseContext::new(Arc::new(NoPersistenceDatabaseCollabService { client_id: default_client_id() }));
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
  view_id: &Uuid,
  database_id: &Uuid,
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
  view_id: &Uuid,
  database_id: &Uuid,
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
  view_id: &Uuid,
  database_id: &Uuid,
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

pub async fn append_block_at_the_end_of_page(
  appflowy_web_metrics: &AppFlowyWebMetrics,
  server: Data<RealtimeServerAddr>,
  user: RealtimeUser,
  collab_storage: &CollabAccessControlStorage,
  workspace_id: Uuid,
  view_id: &str,
  serde_blocks: &[SerdeBlock],
) -> Result<(), AppError> {
  let oid = Uuid::parse_str(view_id)?;
  let update =
    append_block_to_document_collab(user.uid, collab_storage, workspace_id, oid, serde_blocks)
      .await?;
  update_page_collab_data(
    appflowy_web_metrics,
    server,
    user,
    workspace_id,
    oid,
    CollabType::Document,
    update,
  )
  .await
}

async fn append_block_to_document_collab(
  uid: i64,
  collab_storage: &CollabAccessControlStorage,
  workspace_id: Uuid,
  oid: Uuid,
  serde_blocks: &[SerdeBlock],
) -> Result<Vec<u8>, AppError> {
  let original_doc_state = get_latest_collab_encoded(
    collab_storage,
    GetCollabOrigin::User { uid },
    workspace_id,
    oid,
    CollabType::Document,
  )
  .await?
  .doc_state;
  let mut collab = collab_from_doc_state(original_doc_state.to_vec(), &oid, default_client_id())?;
  let document_body = DocumentBody::from_collab(&collab)
    .ok_or_else(|| AppError::Internal(anyhow::anyhow!("invalid document collab")))?;
  let document_data = {
    let txn = collab.transact();
    document_body
      .get_document_data(&txn)
      .map_err(|err| AppError::Internal(anyhow::anyhow!(err.to_string())))
  }?;
  let page_id = document_data.page_id.clone();
  let page_id_children_id = document_data
    .blocks
    .get(&page_id)
    .map(|block| block.children.clone());
  let mut prev_id = page_id_children_id
    .and_then(|children_id| document_data.meta.children_map.get(&children_id))
    .and_then(|child_ids| child_ids.last().cloned());

  let update = {
    let mut txn = collab.transact_mut();
    for serde_block in serde_blocks {
      let (block_index_map, text_map) =
        JsonToDocumentParser::generate_blocks(serde_block, None, page_id.clone());

      for (block_id, block) in block_index_map.iter() {
        document_body
          .insert_block(&mut txn, block.clone(), prev_id.clone())
          .map_err(|err| AppError::InvalidBlock(err.to_string()))?;
        prev_id = Some(block_id.clone());
      }

      for (text_id, text) in text_map.iter() {
        let delta = serde_json::from_str(text).unwrap_or_else(|_| vec![]);
        document_body
          .text_operation
          .apply_delta(&mut txn, text_id, delta);
      }
    }
    txn.encode_update_v1()
  };
  Ok(update)
}

#[allow(clippy::too_many_arguments)]
async fn add_new_space_to_folder(
  uid: i64,
  workspace_id: &Uuid,
  view_id: &Uuid,
  folder: &mut Folder,
  space_permission: &SpacePermission,
  name: &str,
  space_icon: &str,
  space_icon_color: &str,
) -> Result<Vec<u8>, AppError> {
  let encoded_update = {
    let view = NestedChildViewBuilder::new(uid, workspace_id.to_string())
      .with_view_id(view_id)
      .with_name(name)
      .with_extra(|builder| {
        builder
          .with_space_info(SpaceInfo {
            space_icon: Some(space_icon.to_string()),
            space_icon_color: Some(space_icon_color.to_string()),
            space_permission: to_space_permission(space_permission),
            ..Default::default()
          })
          .build()
      })
      .build()
      .view;
    let mut txn = folder.collab.transact_mut();
    folder.body.views.insert(&mut txn, view, None);
    if *space_permission == SpacePermission::Private {
      folder
        .body
        .views
        .update_view(&mut txn, &view_id.to_string(), |update| {
          update.set_private(true).done()
        });
    }
    txn.encode_update_v1()
  };
  Ok(encoded_update)
}

async fn update_space_properties(
  view_id: &str,
  folder: &mut Folder,
  space_permission: &SpacePermission,
  name: &str,
  space_icon: &str,
  space_icon_color: &str,
) -> Result<Vec<u8>, AppError> {
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
  Ok(encoded_update)
}

async fn add_new_database_view_for_workspace_database(
  workspace_database: &mut WorkspaceDatabase,
  database_id: &str,
  view_id: &Uuid,
) -> Result<Vec<u8>, AppError> {
  let encoded_update = {
    let mut txn = workspace_database.collab.transact_mut();
    workspace_database
      .body
      .update_database(&mut txn, database_id, |record| {
        // Check if the view is already linked to the database.
        if !record.linked_views.contains(&view_id.to_string()) {
          record.linked_views.push(view_id.to_string());
        }
      });
    txn.encode_update_v1()
  };
  Ok(encoded_update)
}

async fn add_new_database_to_workspace(
  workspace_database: &mut WorkspaceDatabase,
  database_id: &Uuid,
  view_id: &Uuid,
) -> Result<Vec<u8>, AppError> {
  let encoded_updates = {
    let mut txn = workspace_database.collab.transact_mut();
    workspace_database.body.add_database(
      &mut txn,
      &database_id.to_string(),
      vec![view_id.to_string()],
    );
    txn.encode_update_v1()
  };
  Ok(encoded_updates)
}

async fn add_new_view_to_folder(
  uid: i64,
  parent_view_id: &Uuid,
  view_id: &Uuid,
  folder: &mut Folder,
  name: Option<&str>,
  layout: collab_folder::ViewLayout,
) -> Result<Vec<u8>, AppError> {
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
  Ok(encoded_update)
}

async fn update_favorite_view(
  view_id: &str,
  folder: &mut Folder,
  is_favorite: bool,
  is_pinned: bool,
) -> Result<Vec<u8>, AppError> {
  let existing_extra: Option<serde_json::Value> = folder
    .get_view(view_id)
    .ok_or_else(|| {
      AppError::Internal(anyhow::anyhow!(
        "Failed to find view with id {} in folder",
        view_id
      ))
    })?
    .extra
    .as_ref()
    .map(|extra| serde_json::from_str(extra))
    .transpose()?;
  let extra = if let Some(mut existing_extra) = existing_extra {
    existing_extra["is_pinned"] = serde_json::Value::Bool(is_pinned);
    existing_extra.to_string()
  } else {
    json!({"is_pinned": is_pinned}).to_string().to_string()
  };

  let encoded_update = {
    let mut txn = folder.collab.transact_mut();
    folder.body.views.update_view(&mut txn, view_id, |update| {
      update.set_favorite(is_favorite).set_extra(extra).done()
    });
    txn.encode_update_v1()
  };
  Ok(encoded_update)
}

async fn reorder_favorite_section(
  view_id: &str,
  prev_view_id: Option<&str>,
  folder: &mut Folder,
) -> Result<Vec<u8>, AppError> {
  let encoded_update = {
    let mut txn = folder.collab.transact_mut();
    if let Some(op) = folder
      .body
      .section
      .section_op(&txn, collab_folder::Section::Favorite)
    {
      op.move_section_item_with_txn(&mut txn, view_id, prev_view_id);
    };
    txn.encode_update_v1()
  };

  Ok(encoded_update)
}

async fn update_view_properties(
  view_id: &str,
  folder: &mut Folder,
  name: &str,
  icon: Option<&ViewIcon>,
  is_locked: Option<bool>,
  extra: Option<impl AsRef<str>>,
) -> Result<Vec<u8>, AppError> {
  let encoded_update = {
    let mut txn = folder.collab.transact_mut();
    let icon = icon.map(|icon| to_folder_view_icon(icon.clone()));
    folder.body.views.update_view(&mut txn, view_id, |update| {
      update
        .set_name(name)
        .set_icon(icon)
        .set_extra_if_not_none(extra)
        .set_is_locked(is_locked)
        .done()
    });
    txn.encode_update_v1()
  };
  Ok(encoded_update)
}

async fn update_view_name(
  view_id: &str,
  folder: &mut Folder,
  name: &str,
) -> Result<Vec<u8>, AppError> {
  let encoded_update = {
    let mut txn = folder.collab.transact_mut();
    folder
      .body
      .views
      .update_view(&mut txn, view_id, |update| update.set_name(name).done());
    txn.encode_update_v1()
  };
  Ok(encoded_update)
}

async fn update_view_icon(
  view_id: &str,
  folder: &mut Folder,
  icon: Option<&ViewIcon>,
) -> Result<Vec<u8>, AppError> {
  let encoded_update = {
    let mut txn = folder.collab.transact_mut();
    let icon = icon.map(|icon| to_folder_view_icon(icon.clone()));
    folder
      .body
      .views
      .update_view(&mut txn, view_id, |update| update.set_icon(icon).done());
    txn.encode_update_v1()
  };
  Ok(encoded_update)
}

async fn update_view_extra(
  view_id: &str,
  folder: &mut Folder,
  extra: &str,
) -> Result<Vec<u8>, AppError> {
  let encoded_update = {
    let mut txn = folder.collab.transact_mut();
    folder
      .body
      .views
      .update_view(&mut txn, view_id, |update| update.set_extra(extra).done());
    txn.encode_update_v1()
  };
  Ok(encoded_update)
}

async fn move_view(
  view_id: &str,
  new_parent_view_id: &str,
  prev_view_id: Option<String>,
  folder: &mut Folder,
) -> Result<Vec<u8>, AppError> {
  let encoded_update = {
    let mut txn = folder.collab.transact_mut();
    folder
      .body
      .move_nested_view(&mut txn, view_id, new_parent_view_id, prev_view_id);
    txn.encode_update_v1()
  };
  Ok(encoded_update)
}

async fn move_view_to_trash(view_id: &str, folder: &mut Folder) -> Result<Vec<u8>, AppError> {
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
  Ok(encoded_update)
}

async fn move_view_out_from_trash(view_id: &str, folder: &mut Folder) -> Result<Vec<u8>, AppError> {
  let encoded_update = {
    let mut txn = folder.collab.transact_mut();
    folder
      .body
      .views
      .update_view(&mut txn, view_id, |update| update.set_trash(false).done());
    txn.encode_update_v1()
  };
  Ok(encoded_update)
}

async fn extend_recent_views(
  recent_view_ids: &[String],
  folder: &mut Folder,
) -> Result<Vec<u8>, AppError> {
  let existing_recent_sections: HashSet<String> = folder
    .get_all_recent_sections()
    .iter()
    .map(|s| s.id.clone())
    .collect();
  let section_id_to_be_removed = existing_recent_sections
    .intersection(&recent_view_ids.iter().cloned().collect())
    .cloned()
    .collect_vec();
  let section_item_to_be_added = recent_view_ids
    .iter()
    .map(|id| SectionItem::new(id.clone()))
    .collect_vec();
  let encoded_update = {
    let mut txn = folder.collab.transact_mut();
    if let Some(op) = folder
      .body
      .section
      .section_op(&txn, collab_folder::Section::Recent)
    {
      op.delete_section_items_with_txn(&mut txn, section_id_to_be_removed);
      op.add_sections_item(&mut txn, section_item_to_be_added);
    };
    txn.encode_update_v1()
  };

  Ok(encoded_update)
}

async fn move_all_views_out_from_trash(folder: &mut Folder) -> Result<Vec<u8>, AppError> {
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

  Ok(encoded_update)
}

async fn delete_view_from_trash(view_id: &str, folder: &mut Folder) -> Result<Vec<u8>, AppError> {
  let encoded_update = {
    let mut txn = folder.collab.transact_mut();
    folder
      .body
      .views
      .update_view(&mut txn, view_id, |update| update.set_trash(false).done());
    folder.body.views.delete_views(&mut txn, vec![view_id]);
    txn.encode_update_v1()
  };

  Ok(encoded_update)
}

async fn delete_all_views_from_trash(folder: &mut Folder) -> Result<Vec<u8>, AppError> {
  let all_trash_ids: Vec<String> = folder
    .get_all_trash_sections()
    .iter()
    .map(|s| s.id.clone())
    .collect();

  let encoded_update = {
    let mut txn = folder.collab.transact_mut();
    if let Some(op) = folder
      .body
      .section
      .section_op(&txn, collab_folder::Section::Trash)
    {
      op.clear(&mut txn);
    };
    folder.body.views.delete_views(&mut txn, all_trash_ids);
    txn.encode_update_v1()
  };

  Ok(encoded_update)
}

#[allow(clippy::too_many_arguments)]
async fn create_document_page(
  appflowy_web_metrics: &AppFlowyWebMetrics,
  server: Data<RealtimeServerAddr>,
  user: RealtimeUser,
  pg_pool: &PgPool,
  collab_storage: &CollabAccessControlStorage,
  workspace_id: Uuid,
  parent_view_id: &Uuid,
  name: Option<&str>,
  page_data: Option<&serde_json::Value>,
  view_id_override: Option<Uuid>,
  collab_id_override: Option<Uuid>,
) -> Result<Page, AppError> {
  let client_id = default_client_id();
  let collab_id = collab_id_override.unwrap_or(Uuid::new_v4());

  let new_document_collab_params = match page_data {
    Some(page_data) => {
      prepare_document_collab_param_with_initial_data(client_id, page_data.clone(), collab_id)
    },
    None => prepare_default_document_collab_param(client_id, collab_id),
  }?;
  let view_id = view_id_override.unwrap_or(collab_id);
  let collab_origin = GetCollabOrigin::User { uid: user.uid };
  let mut folder = get_latest_collab_folder(collab_storage, collab_origin, workspace_id, client_id).await?;
  let folder_update = add_new_view_to_folder(
    user.uid,
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
      workspace_id,
      &user.uid,
      new_document_collab_params,
      &mut transaction,
      &action,
    )
    .await?;
  update_workspace_folder_data(
    appflowy_web_metrics,
    server,
    user,
    workspace_id,
    folder_update,
  )
  .await?;
  transaction.commit().await?;
  collab_storage.metrics().observe_pg_tx(start.elapsed());
  Ok(Page { view_id })
}

#[allow(clippy::too_many_arguments)]
async fn create_grid_page(
  appflowy_web_metrics: &AppFlowyWebMetrics,
  server: Data<RealtimeServerAddr>,
  user: RealtimeUser,
  pg_pool: &PgPool,
  collab_storage: &CollabAccessControlStorage,
  workspace_id: Uuid,
  parent_view_id: &Uuid,
  name: Option<&str>,
) -> Result<Page, AppError> {
  let view_id = Uuid::new_v4();
  let database_id: Uuid = gen_database_id().parse().unwrap();
  let default_grid_encoded_database =
    prepare_default_grid_encoded_database(&view_id, &database_id, name.unwrap_or_default()).await?;
  create_database_page(
    appflowy_web_metrics,
    server,
    user,
    pg_pool,
    collab_storage,
    workspace_id,
    parent_view_id,
    &view_id,
    collab_folder::ViewLayout::Grid,
    name,
    &default_grid_encoded_database,
  )
  .await
}

#[allow(clippy::too_many_arguments)]
async fn create_board_page(
  appflowy_web_metrics: &AppFlowyWebMetrics,
  server: Data<RealtimeServerAddr>,
  user: RealtimeUser,
  pg_pool: &PgPool,
  collab_storage: &CollabAccessControlStorage,
  workspace_id: Uuid,
  parent_view_id: &Uuid,
  name: Option<&str>,
) -> Result<Page, AppError> {
  let view_id = Uuid::new_v4();
  let database_id = Uuid::new_v4();
  let default_board_encoded_database =
    prepare_default_board_encoded_database(&view_id, &database_id, name.unwrap_or_default())
      .await?;
  create_database_page(
    appflowy_web_metrics,
    server,
    user,
    pg_pool,
    collab_storage,
    workspace_id,
    parent_view_id,
    &view_id,
    collab_folder::ViewLayout::Board,
    name,
    &default_board_encoded_database,
  )
  .await
}

#[allow(clippy::too_many_arguments)]
async fn create_calendar_page(
  appflowy_web_metrics: &AppFlowyWebMetrics,
  server: Data<RealtimeServerAddr>,
  user: RealtimeUser,
  pg_pool: &PgPool,
  collab_storage: &CollabAccessControlStorage,
  workspace_id: Uuid,
  parent_view_id: &Uuid,
  name: Option<&str>,
) -> Result<Page, AppError> {
  let view_id = Uuid::new_v4();
  let database_id = Uuid::new_v4();
  let default_calendar_encoded_database =
    prepare_default_calendar_encoded_database(&view_id, &database_id, name.unwrap_or_default())
      .await?;
  create_database_page(
    appflowy_web_metrics,
    server,
    user,
    pg_pool,
    collab_storage,
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
  appflowy_web_metrics: &AppFlowyWebMetrics,
  server: Data<RealtimeServerAddr>,
  user: RealtimeUser,
  pg_pool: &PgPool,
  collab_storage: &CollabAccessControlStorage,
  workspace_id: Uuid,
  parent_view_id: &Uuid,
  view_id: &Uuid,
  view_layout: collab_folder::ViewLayout,
  name: Option<&str>,
  encoded_database: &EncodedDatabase,
) -> Result<Page, AppError> {
  let collab_origin = GetCollabOrigin::User { uid: user.uid };
  let mut folder =
    get_latest_collab_folder(collab_storage, collab_origin.clone(), workspace_id, default_client_id()).await?;
  let folder_update = add_new_view_to_folder(
    user.uid,
    parent_view_id,
    view_id,
    &mut folder,
    name,
    view_layout,
  )
  .await?;
  let (workspace_database_id, mut workspace_database) =
    get_latest_workspace_database(collab_storage, pg_pool, collab_origin, workspace_id).await?;
  let database_id: Uuid = encoded_database.encoded_database_collab.object_id.parse()?;
  let workspace_database_update =
    add_new_database_to_workspace(&mut workspace_database, &database_id, view_id).await?;
  let database_collab_params = CollabParams {
    object_id: database_id,
    encoded_collab_v1: encoded_database
      .encoded_database_collab
      .encoded_collab
      .encode_to_bytes()?
      .into(),
    collab_type: CollabType::Database,
    updated_at: None,
  };
  let row_collab_params_list = encoded_database
    .encoded_row_collabs
    .iter()
    .flat_map(|row_collab| {
      Some(CollabParams {
        object_id: Uuid::parse_str(&row_collab.object_id).ok()?,
        encoded_collab_v1: row_collab.encoded_collab.encode_to_bytes().unwrap().into(),
        collab_type: CollabType::DatabaseRow,
        updated_at: None,
      })
    })
    .collect();

  let mut transaction = pg_pool.begin().await?;
  let start = Instant::now();
  let action = format!("Create new database collab: {}", database_id);
  collab_storage
    .upsert_new_collab_with_transaction(
      workspace_id,
      &user.uid,
      database_collab_params,
      &mut transaction,
      &action,
    )
    .await?;
  collab_storage
    .batch_insert_new_collab(workspace_id, &user.uid, row_collab_params_list)
    .await?;
  update_workspace_folder_data(
    appflowy_web_metrics,
    server.clone(),
    user.clone(),
    workspace_id,
    folder_update,
  )
  .await?;
  update_workspace_database_data(
    appflowy_web_metrics,
    server,
    user,
    workspace_id,
    workspace_database_id,
    workspace_database_update,
  )
  .await?;
  transaction.commit().await?;
  collab_storage.metrics().observe_pg_tx(start.elapsed());
  Ok(Page { view_id: *view_id })
}

async fn get_rag_ids(folder: &Folder, parent_view_id: &Uuid) -> Vec<Uuid> {
  let parent_view_id_str = parent_view_id.to_string();
  if let Some(view) = folder.get_view(&parent_view_id_str) {
    if view.space_info().is_some() {
      return vec![];
    }
  };
  let trash_ids: HashSet<String> = folder
    .get_all_trash_sections()
    .iter()
    .map(|s| s.id.clone())
    .collect();
  let mut rag_ids: Vec<_> = folder
    .get_views_belong_to(&parent_view_id_str)
    .iter()
    .filter(|v| v.layout.is_document() && !trash_ids.contains(&v.id))
    .flat_map(|v| Uuid::parse_str(&v.id).ok())
    .collect();
  rag_ids.push(*parent_view_id);
  rag_ids
}

#[allow(clippy::too_many_arguments)]
async fn create_chat_page(
  appflowy_web_metrics: &AppFlowyWebMetrics,
  server: Data<RealtimeServerAddr>,
  user: RealtimeUser,
  pg_pool: &PgPool,
  collab_storage: &CollabAccessControlStorage,
  workspace_id: Uuid,
  parent_view_id: &Uuid,
  name: Option<&str>,
) -> Result<Page, AppError> {
  let view_id = Uuid::new_v4();
  let collab_origin = GetCollabOrigin::User { uid: user.uid };
  let mut folder =
    get_latest_collab_folder(collab_storage, collab_origin.clone(), workspace_id, default_client_id()).await?;
  let rag_ids = get_rag_ids(&folder, parent_view_id).await;
  create_chat(
    pg_pool,
    CreateChatParams {
      chat_id: view_id.to_string(),
      name: name.unwrap_or_default().to_string(),
      rag_ids,
    },
    &workspace_id,
  )
  .await?;
  let folder_update = add_new_view_to_folder(
    user.uid,
    parent_view_id,
    &view_id,
    &mut folder,
    name,
    collab_folder::ViewLayout::Chat,
  )
  .await?;
  update_workspace_folder_data(
    appflowy_web_metrics,
    server.clone(),
    user.clone(),
    workspace_id,
    folder_update,
  )
  .await?;
  Ok(Page { view_id })
}

#[allow(clippy::too_many_arguments)]
pub async fn move_page(
  appflowy_web_metrics: &AppFlowyWebMetrics,
  server: Data<RealtimeServerAddr>,
  user: RealtimeUser,
  collab_storage: &CollabAccessControlStorage,
  workspace_id: Uuid,
  view_id: &str,
  new_parent_view_id: &str,
  prev_view_id: Option<String>,
) -> Result<(), AppError> {
  let collab_origin = GetCollabOrigin::User { uid: user.uid };
  let mut folder = get_latest_collab_folder(collab_storage, collab_origin, workspace_id, default_client_id()).await?;
  let folder_update = move_view(view_id, new_parent_view_id, prev_view_id, &mut folder).await?;
  update_workspace_folder_data(
    appflowy_web_metrics,
    server,
    user,
    workspace_id,
    folder_update,
  )
  .await?;
  Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn reorder_favorite_page(
  appflowy_web_metrics: &AppFlowyWebMetrics,
  server: Data<RealtimeServerAddr>,
  user: RealtimeUser,
  collab_storage: &CollabAccessControlStorage,
  workspace_id: Uuid,
  view_id: &str,
  prev_view_id: Option<&str>,
) -> Result<(), AppError> {
  let collab_origin = GetCollabOrigin::User { uid: user.uid };
  let mut folder = get_latest_collab_folder(collab_storage, collab_origin, workspace_id, default_client_id()).await?;
  let folder_update = reorder_favorite_section(view_id, prev_view_id, &mut folder).await?;
  update_workspace_folder_data(
    appflowy_web_metrics,
    server,
    user,
    workspace_id,
    folder_update,
  )
  .await?;
  Ok(())
}

pub async fn move_page_to_trash(
  appflowy_web_metrics: &AppFlowyWebMetrics,
  server: Data<RealtimeServerAddr>,
  user: RealtimeUser,
  collab_storage: &CollabAccessControlStorage,
  workspace_id: Uuid,
  view_id: &str,
) -> Result<(), AppError> {
  let collab_origin = GetCollabOrigin::User { uid: user.uid };
  let mut folder = get_latest_collab_folder(collab_storage, collab_origin, workspace_id, default_client_id()).await?;
  let trash_info = folder.get_my_trash_info();
  if trash_info.into_iter().any(|info| info.id == view_id) {
    return Ok(());
  }
  let folder_update = move_view_to_trash(view_id, &mut folder).await?;
  update_workspace_folder_data(
    appflowy_web_metrics,
    server,
    user,
    workspace_id,
    folder_update,
  )
  .await?;
  Ok(())
}

pub async fn restore_page_from_trash(
  appflowy_web_metrics: &AppFlowyWebMetrics,
  server: Data<RealtimeServerAddr>,
  user: RealtimeUser,
  collab_storage: &CollabAccessControlStorage,
  workspace_id: Uuid,
  view_id: &str,
) -> Result<(), AppError> {
  let collab_origin = GetCollabOrigin::User { uid: user.uid };
  let mut folder = get_latest_collab_folder(collab_storage, collab_origin, workspace_id, default_client_id()).await?;
  let folder_update = move_view_out_from_trash(view_id, &mut folder).await?;
  update_workspace_folder_data(
    appflowy_web_metrics,
    server,
    user,
    workspace_id,
    folder_update,
  )
  .await?;
  Ok(())
}

pub async fn add_recent_pages(
  appflowy_web_metrics: &AppFlowyWebMetrics,
  server: Data<RealtimeServerAddr>,
  user: RealtimeUser,
  collab_storage: &CollabAccessControlStorage,
  workspace_id: Uuid,
  recent_view_ids: Vec<String>,
) -> Result<(), AppError> {
  let collab_origin = GetCollabOrigin::User { uid: user.uid };
  let mut folder = get_latest_collab_folder(collab_storage, collab_origin, workspace_id, default_client_id()).await?;
  let folder_update = extend_recent_views(&recent_view_ids, &mut folder).await?;
  update_workspace_folder_data(
    appflowy_web_metrics,
    server,
    user,
    workspace_id,
    folder_update,
  )
  .await?;
  Ok(())
}

pub async fn restore_all_pages_from_trash(
  appflowy_web_metrics: &AppFlowyWebMetrics,
  server: Data<RealtimeServerAddr>,
  user: RealtimeUser,
  collab_storage: &CollabAccessControlStorage,
  workspace_id: Uuid,
) -> Result<(), AppError> {
  let collab_origin = GetCollabOrigin::User { uid: user.uid };
  let mut folder = get_latest_collab_folder(collab_storage, collab_origin, workspace_id, default_client_id()).await?;
  let folder_update = move_all_views_out_from_trash(&mut folder).await?;
  update_workspace_folder_data(
    appflowy_web_metrics,
    server,
    user,
    workspace_id,
    folder_update,
  )
  .await?;
  Ok(())
}

pub async fn delete_trash(
  appflowy_web_metrics: &AppFlowyWebMetrics,
  server: Data<RealtimeServerAddr>,
  user: RealtimeUser,
  collab_storage: &CollabAccessControlStorage,
  workspace_id: Uuid,
  view_id: &str,
) -> Result<(), AppError> {
  let uid = user.uid;
  let collab_origin = GetCollabOrigin::User { uid };
  let mut folder = get_latest_collab_folder(collab_storage, collab_origin, workspace_id, default_client_id()).await?;
  let update = delete_view_from_trash(view_id, &mut folder).await?;
  update_workspace_folder_data(appflowy_web_metrics, server, user, workspace_id, update).await?;
  Ok(())
}

pub async fn delete_all_pages_from_trash(
  appflowy_web_metrics: &AppFlowyWebMetrics,
  server: Data<RealtimeServerAddr>,
  user: RealtimeUser,
  collab_storage: &CollabAccessControlStorage,
  workspace_id: Uuid,
) -> Result<(), AppError> {
  let uid = user.uid;
  let collab_origin = GetCollabOrigin::User { uid };
  let mut folder = get_latest_collab_folder(collab_storage, collab_origin, workspace_id, default_client_id()).await?;
  let update = delete_all_views_from_trash(&mut folder).await?;
  update_workspace_folder_data(appflowy_web_metrics, server, user, workspace_id, update).await?;
  Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn update_page(
  appflowy_web_metrics: &AppFlowyWebMetrics,
  server: Data<RealtimeServerAddr>,
  user: RealtimeUser,
  collab_storage: &CollabAccessControlStorage,
  workspace_id: Uuid,
  view_id: &str,
  name: &str,
  icon: Option<&ViewIcon>,
  is_locked: Option<bool>,
  extra: Option<impl AsRef<str>>,
) -> Result<(), AppError> {
  let collab_origin = GetCollabOrigin::User { uid: user.uid };
  let mut folder = get_latest_collab_folder(collab_storage, collab_origin, workspace_id, default_client_id()).await?;
  let folder_update =
    update_view_properties(view_id, &mut folder, name, icon, is_locked, extra).await?;
  update_workspace_folder_data(
    appflowy_web_metrics,
    server,
    user,
    workspace_id,
    folder_update,
  )
  .await?;

  Ok(())
}

pub async fn update_page_name(
  appflowy_web_metrics: &AppFlowyWebMetrics,
  server: Data<RealtimeServerAddr>,
  user: RealtimeUser,
  collab_storage: &CollabAccessControlStorage,
  workspace_id: Uuid,
  view_id: &str,
  name: &str,
) -> Result<(), AppError> {
  let collab_origin = GetCollabOrigin::User { uid: user.uid };
  let mut folder = get_latest_collab_folder(collab_storage, collab_origin, workspace_id, default_client_id()).await?;
  let folder_update = update_view_name(view_id, &mut folder, name).await?;
  update_workspace_folder_data(
    appflowy_web_metrics,
    server,
    user,
    workspace_id,
    folder_update,
  )
  .await?;

  Ok(())
}

pub async fn update_page_icon(
  appflowy_web_metrics: &AppFlowyWebMetrics,
  server: Data<RealtimeServerAddr>,
  user: RealtimeUser,
  collab_storage: &CollabAccessControlStorage,
  workspace_id: Uuid,
  view_id: &str,
  icon: Option<&ViewIcon>,
) -> Result<(), AppError> {
  let collab_origin = GetCollabOrigin::User { uid: user.uid };
  let mut folder = get_latest_collab_folder(collab_storage, collab_origin, workspace_id, default_client_id()).await?;
  let folder_update = update_view_icon(view_id, &mut folder, icon).await?;
  update_workspace_folder_data(
    appflowy_web_metrics,
    server,
    user,
    workspace_id,
    folder_update,
  )
  .await?;

  Ok(())
}

pub async fn update_page_extra(
  appflowy_web_metrics: &AppFlowyWebMetrics,
  server: Data<RealtimeServerAddr>,
  user: RealtimeUser,
  collab_storage: &CollabAccessControlStorage,
  workspace_id: Uuid,
  view_id: &str,
  extra: &str,
) -> Result<(), AppError> {
  let collab_origin = GetCollabOrigin::User { uid: user.uid };
  let mut folder = get_latest_collab_folder(collab_storage, collab_origin, workspace_id, default_client_id()).await?;
  let folder_update = update_view_extra(view_id, &mut folder, extra).await?;
  update_workspace_folder_data(
    appflowy_web_metrics,
    server,
    user,
    workspace_id,
    folder_update,
  )
  .await?;

  Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn favorite_page(
  appflowy_web_metrics: &AppFlowyWebMetrics,
  server: Data<RealtimeServerAddr>,
  user: RealtimeUser,
  collab_storage: &CollabAccessControlStorage,
  workspace_id: Uuid,
  view_id: &str,
  is_favorite: bool,
  is_pinned: bool,
) -> Result<(), AppError> {
  let collab_origin = GetCollabOrigin::User { uid: user.uid };
  let mut folder = get_latest_collab_folder(collab_storage, collab_origin, workspace_id, default_client_id()).await?;
  let folder_update = update_favorite_view(view_id, &mut folder, is_favorite, is_pinned).await?;
  update_workspace_folder_data(
    appflowy_web_metrics,
    server,
    user,
    workspace_id,
    folder_update,
  )
  .await?;

  Ok(())
}

static INVALID_URL_CHARS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[^\w-]").unwrap());

fn replace_invalid_url_chars(input: &str) -> String {
  INVALID_URL_CHARS.replace_all(input, "-").to_string()
}

fn generate_publish_name(view_id: &str, name: &str) -> String {
  let id_len = view_id.len();
  let name = replace_invalid_url_chars(name);
  let name_len = name.len();
  // The backend limits the publish name to a maximum of 50 characters.
  // If the combined length of the ID and the name exceeds 50 characters,
  // we will truncate the name to ensure the final result is within the limit.
  // The name should only contain alphanumeric characters and hyphens.
  let result = format!(
    "{}-{}",
    &name[..std::cmp::min(49 - id_len, name_len)],
    view_id
  );
  result
}

#[allow(clippy::too_many_arguments)]
pub async fn publish_page(
  pg_pool: &PgPool,
  collab_access_control_storage: &CollabAccessControlStorage,
  publish_collab_store: &dyn PublishedCollabStore,
  uid: i64,
  user_uuid: Uuid,
  workspace_id: Uuid,
  view_id: Uuid,
  visible_database_view_ids: Option<Vec<Uuid>>,
  publish_name: Option<impl ToString>,
  comments_enabled: bool,
  duplicate_enabled: bool,
) -> Result<(), AppError> {
  let folder = get_latest_collab_folder(
    collab_access_control_storage,
    GetCollabOrigin::User { uid },
    workspace_id,
    default_client_id()
  )
  .await?;
  let view = folder
    .get_view(&view_id.to_string())
    .ok_or(AppError::InvalidFolderView(format!(
      "View {} not found",
      view_id
    )))?;
  let icon = view
    .icon
    .as_ref()
    .map(|icon| to_dto_view_icon(icon.clone()));
  let metadata = PublishViewMetaData {
    view: PublishViewInfo {
      view_id: view_id.to_string(),
      name: view.name.clone(),
      icon,
      layout: to_dto_view_layout(&view.layout),
      extra: view.extra.clone(),
      created_by: view.created_by,
      last_edited_by: view.last_edited_by,
      last_edited_time: view.last_edited_time,
      created_at: view.created_at,
      child_views: None,
    },
    // Note: The use of child views and ancestor views are going to be deprecated in
    // appflowy web as there is now endpoint to obtain published outline.
    child_views: vec![],
    ancestor_views: vec![],
  };

  let publish_data = match view.layout {
    collab_folder::ViewLayout::Document => {
      generate_publish_data_for_document(collab_access_control_storage, uid, workspace_id, view_id)
        .await
    },
    collab_folder::ViewLayout::Grid
    | collab_folder::ViewLayout::Board
    | collab_folder::ViewLayout::Calendar => {
      generate_publish_data_for_database(
        pg_pool,
        collab_access_control_storage,
        uid,
        workspace_id,
        view_id,
        visible_database_view_ids,
      )
      .await
    },
    collab_folder::ViewLayout::Chat => Err(AppError::InvalidRequest(
      "AI Chat cannot be published".to_string(),
    )),
  }?;
  publish_collab_store
    .publish_collabs(
      vec![PublishCollabItem {
        meta: PublishCollabMetadata {
          view_id,
          publish_name: publish_name
            .map(|name| name.to_string())
            .unwrap_or_else(|| generate_publish_name(&view.id, &view.name)),
          metadata: serde_json::value::to_value(metadata).unwrap(),
        },
        data: publish_data,
        comments_enabled,
        duplicate_enabled,
      }],
      &workspace_id,
      &user_uuid,
    )
    .await?;
  Ok(())
}

async fn generate_publish_data_for_document(
  collab_access_control_storage: &CollabAccessControlStorage,
  uid: i64,
  workspace_id: Uuid,
  view_id: Uuid,
) -> Result<Vec<u8>, AppError> {
  let collab = get_latest_collab_encoded(
    collab_access_control_storage,
    GetCollabOrigin::User { uid },
    workspace_id,
    view_id,
    CollabType::Document,
  )
  .await?;
  Ok(collab.doc_state.to_vec())
}

async fn generate_publish_data_for_database(
  pg_pool: &PgPool,
  collab_storage: &CollabAccessControlStorage,
  uid: i64,
  workspace_id: Uuid,
  view_id: Uuid,
  visible_database_view_ids: Option<Vec<Uuid>>,
) -> Result<Vec<u8>, AppError> {
  let (_, ws_db) = get_latest_workspace_database(
    collab_storage,
    pg_pool,
    GetCollabOrigin::User { uid },
    workspace_id,
  )
  .await?;
  let db_oid = {
    ws_db
      .get_database_meta_with_view_id(&view_id.to_string())
      .ok_or(AppError::NoRequiredData(format!(
        "Database view {} not found",
        view_id
      )))?
      .database_id
  };
  let db_oid = Uuid::parse_str(&db_oid)?;
  let (db_collab, db_body) =
    get_latest_collab_database_body(collab_storage, workspace_id, db_oid).await?;
  let inline_view_id = {
    let txn = db_collab.transact();
    db_body.get_inline_view_id(&txn)
  };
  let row_ids: Vec<_> = {
    let txn = db_collab.transact();
    db_body
      .views
      .get_row_orders(&txn, &inline_view_id)
      .iter()
      .flat_map(|ro| Uuid::parse_str(&ro.id))
      .collect()
  };
  let encoded_rows = batch_get_latest_collab_encoded(
    collab_storage,
    GetCollabOrigin::User { uid },
    workspace_id,
    &row_ids,
    CollabType::DatabaseRow,
  )
  .await?;
  let row_data: HashMap<_, Vec<u8>> = encoded_rows
    .into_iter()
    .map(|(oid, encoded_collab)| (oid, encoded_collab.doc_state.to_vec()))
    .collect();

  let row_document_ids: Vec<_> = row_ids
    .iter()
    .filter_map(|row_id| {
      db_body
        .block
        .get_row_document_id(&RowId::from(row_id.to_owned()))
        .and_then(|doc_id| Uuid::parse_str(&doc_id).ok())
    })
    .collect();
  let encoded_row_documents = batch_get_latest_collab_encoded(
    collab_storage,
    GetCollabOrigin::User { uid },
    workspace_id,
    &row_document_ids,
    CollabType::Document,
  )
  .await?;
  let row_document_data: HashMap<_, _> = encoded_row_documents
    .into_iter()
    .map(|(oid, encoded_collab)| (oid, encoded_collab.doc_state.to_vec()))
    .collect();

  let data = PublishDatabaseData {
    database_collab: collab_to_doc_state(db_collab, CollabType::Database).await?,
    database_row_collabs: row_data,
    database_row_document_collabs: row_document_data,
    visible_database_view_ids: visible_database_view_ids.unwrap_or(vec![view_id]),
    database_relations: HashMap::from([(db_oid, view_id)]),
  };
  Ok(serde_json::ser::to_vec(&data)?)
}

pub async fn unpublish_page(
  publish_collab_store: &dyn PublishedCollabStore,
  workspace_id: Uuid,
  user_uuid: Uuid,
  view_id: Uuid,
) -> Result<(), AppError> {
  publish_collab_store
    .unpublish_collabs(&workspace_id, &[view_id], &user_uuid)
    .await
}

pub async fn get_page_view_collab(
  pg_pool: &PgPool,
  collab_access_control_storage: &CollabAccessControlStorage,
  uid: i64,
  workspace_id: Uuid,
  view_id: Uuid,
) -> Result<PageCollab, AppError> {
  let folder = get_latest_collab_folder(
    collab_access_control_storage,
    GetCollabOrigin::User { uid },
    workspace_id,
    default_client_id()
  )
  .await?;
  let view = folder
    .get_view(&view_id.to_string())
    .ok_or(AppError::InvalidFolderView(format!(
      "View {} not found",
      view_id
    )))?;

  let owner = match view.created_by {
    Some(uid) => select_web_user_from_uid(pg_pool, uid).await?,
    None => None,
  };
  let last_editor = match view.last_edited_by {
    Some(uid) => select_web_user_from_uid(pg_pool, uid).await?,
    None => None,
  };
  let publish_view_ids = select_published_view_ids_for_workspace(pg_pool, workspace_id)
    .await
    .map_err(|err| {
      AppError::Internal(anyhow::anyhow!(
        "Unable to obtain published view id for workspace {}: {}",
        workspace_id,
        err
      ))
    })?;
  let publish_view_ids: HashSet<_> = publish_view_ids.into_iter().collect();
  let parent_view_id = Uuid::parse_str(&view.parent_view_id).ok();
  let folder_view = FolderView {
    view_id,
    parent_view_id,
    prev_view_id: get_prev_view_id(&folder, &view_id),
    name: view.name.clone(),
    icon: view
      .icon
      .as_ref()
      .map(|icon| to_dto_view_icon(icon.clone())),
    is_space: check_if_view_is_space(&view),
    is_private: false,
    is_favorite: view.is_favorite,
    is_published: publish_view_ids.contains(&view_id),
    layout: to_dto_view_layout(&view.layout),
    created_at: DateTime::from_timestamp(view.created_at, 0).unwrap_or_default(),
    created_by: view.created_by,
    last_edited_by: view.last_edited_by,
    last_edited_time: DateTime::from_timestamp(view.last_edited_time, 0).unwrap_or_default(),
    is_locked: view.is_locked,
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
  view_id: Uuid,
) -> Result<PageCollabData, AppError> {
  let ws_db_oid = select_workspace_database_oid(pg_pool, &workspace_id)
    .await
    .map_err(|err| {
      AppError::Internal(anyhow::anyhow!(
        "Unable to find workspace database oid for {}: {}",
        workspace_id,
        err
      ))
    })?;
  let ws_db = get_latest_collab_encoded(
    collab_access_control_storage,
    GetCollabOrigin::User { uid },
    workspace_id,
    ws_db_oid,
    CollabType::WorkspaceDatabase,
  )
  .await
  .map_err(|err| {
    AppError::Internal(anyhow::anyhow!(
      "Unable to get latest workspace database collab {}: {}",
      &ws_db_oid,
      err
    ))
  })?;
  let ws_db_collab =
    collab_from_doc_state(ws_db.doc_state.to_vec(), &ws_db_oid, default_client_id()).map_err(|err| {
      AppError::Internal(anyhow::anyhow!(
        "Unable to decode workspace database collab {}: {}",
        &ws_db_oid,
        err
      ))
    })?;
  let ws_db_body = WorkspaceDatabase::open(ws_db_collab).map_err(|err| {
    AppError::Internal(anyhow!("Failed to open workspace database body: {}", err))
  })?;
  let db_oid = {
    ws_db_body
      .get_database_meta_with_view_id(&view_id.to_string())
      .ok_or(AppError::NoRequiredData(format!(
        "Database view {} not found",
        view_id
      )))?
      .database_id
  };
  let db = get_latest_collab_encoded(
    collab_access_control_storage,
    GetCollabOrigin::User { uid },
    workspace_id,
    Uuid::parse_str(&db_oid)?,
    CollabType::Database,
  )
  .await
  .map_err(|err| {
    AppError::Internal(anyhow::anyhow!(
      "Unable to get latest database collab {}: {}",
      &db_oid,
      err
    ))
  })?;
  let client_id = default_client_id();
  let options =
    CollabOptions::new(db_oid.to_string(), client_id).with_data_source(db.clone().into());
  let db_collab = Collab::new_with_options(CollabOrigin::Server, options).map_err(|err| {
    AppError::Internal(anyhow!(
      "Unable to create collab from object id {}: {}",
      &db_oid,
      err
    ))
  })?;
  let db_body = DatabaseBody::from_collab(
    &db_collab,
    Arc::new(NoPersistenceDatabaseCollabService { client_id }),
    None,
  )
  .ok_or_else(|| AppError::RecordNotFound("no database body found".to_string()))?;
  let inline_view_id = {
    let txn = db_collab.transact();
    db_body.get_inline_view_id(&txn)
  };
  let row_ids: Vec<_> = {
    let txn = db_collab.transact();
    db_body
      .views
      .get_row_orders(&txn, &inline_view_id)
      .iter()
      .flat_map(|ro| Uuid::parse_str(&ro.id).ok())
      .collect()
  };
  let queries: Vec<QueryCollab> = row_ids
    .iter()
    .map(|row_id| QueryCollab {
      object_id: *row_id,
      collab_type: CollabType::DatabaseRow,
    })
    .collect();
  let row_query_collab_results = collab_access_control_storage
    .batch_get_collab(&uid, workspace_id, queries, true)
    .await;
  let row_data = tokio::task::spawn_blocking(move || {
    let row_collabs: HashMap<_, _> = row_query_collab_results
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
  .await
  .map_err(|err| {
    AppError::Internal(anyhow::anyhow!(
      "Unable to get row data for database {}: {}",
      &db_oid,
      err
    ))
  })?;

  Ok(PageCollabData {
    encoded_collab: db.doc_state.to_vec(),
    row_data,
  })
}

async fn get_page_collab_data_for_document(
  collab_access_control_storage: &CollabAccessControlStorage,
  uid: i64,
  workspace_id: Uuid,
  view_id: Uuid,
) -> Result<PageCollabData, AppError> {
  let collab = get_latest_collab_encoded(
    collab_access_control_storage,
    GetCollabOrigin::User { uid },
    workspace_id,
    view_id,
    CollabType::Document,
  )
  .await
  .map_err(|err| {
    AppError::Internal(anyhow::anyhow!(
      "Unable to get page collab data for {}: {}",
      view_id,
      err
    ))
  })?;
  Ok(PageCollabData {
    encoded_collab: collab.doc_state.clone().to_vec(),
    row_data: HashMap::default(),
  })
}

#[allow(clippy::too_many_arguments)]
pub async fn create_database_view(
  appflowy_web_metrics: &AppFlowyWebMetrics,
  server: Data<RealtimeServerAddr>,
  user: RealtimeUser,
  pg_pool: &PgPool,
  collab_storage: &CollabAccessControlStorage,
  workspace_id: Uuid,
  database_view_id: &Uuid,
  view_layout: &ViewLayout,
  name: Option<&str>,
) -> Result<(), AppError> {
  let database_layout = match view_layout {
    ViewLayout::Grid => DatabaseLayout::Grid,
    ViewLayout::Board => DatabaseLayout::Board,
    ViewLayout::Calendar => DatabaseLayout::Calendar,
    _ => {
      return Err(AppError::InvalidRequest(
        "The layout type is not supported for database view creation".to_string(),
      ))
    },
  };

  let client_id = default_client_id();
  let timestamp = collab_database::database::timestamp();
  let uid = user.uid;
  let collab_origin = GetCollabOrigin::User { uid };
  let (_, workspace_database) =
    get_latest_workspace_database(collab_storage, pg_pool, collab_origin, workspace_id).await?;
  let database_id: Uuid = workspace_database
    .get_database_meta_with_view_id(&database_view_id.to_string())
    .ok_or(AppError::NoRequiredData(format!(
      "Database view {} not found",
      database_view_id
    )))?
    .database_id
    .parse()?;
  let encoded_collab = get_latest_collab_encoded(
    collab_storage,
    GetCollabOrigin::User { uid },
    workspace_id,
    database_id,
    CollabType::Database,
  )
  .await?;
  let options =
    CollabOptions::new(database_id.to_string(), client_id).with_data_source(encoded_collab.into());
  let mut database_collab =
    Collab::new_with_options(CollabOrigin::Server, options).map_err(|err| {
      AppError::Internal(anyhow!(
        "Unable to create collab from object id {}: {}",
        &database_id,
        err
      ))
    })?;

  let database_body = DatabaseBody::from_collab(
    &database_collab,
    Arc::new(NoPersistenceDatabaseCollabService { client_id }),
    None,
  )
  .ok_or_else(|| AppError::RecordNotFound("no database body found".to_string()))?;
  let (row_orders, field_orders, fields) = {
    let txn = database_collab.transact();
    let inline_view_id = database_body.get_inline_view_id(&txn);
    let row_orders = database_body.views.get_row_orders(&txn, &inline_view_id);
    let field_orders = database_body.views.get_field_orders(&txn, &inline_view_id);
    let fields = database_body.fields.get_all_fields(&txn);
    (row_orders, field_orders, fields)
  };
  let LinkedViewDependencies {
    layout_settings,
    field_settings,
    group_settings,
    deps_fields,
  } = resolve_dependencies_when_create_database_linked_view(database_layout, &fields)?;
  let new_view_id = Uuid::new_v4();
  let database_encoded_update = {
    let mut txn = database_collab.transact_mut();
    let deps_field_setting = vec![default_field_settings_by_layout_map()];
    let params = CreateViewParams {
      database_id: database_id.to_string(),
      view_id: new_view_id.to_string(),
      name: name.unwrap_or_default().to_string(),
      layout: database_layout,
      layout_settings,
      filters: vec![],
      group_settings,
      sorts: vec![],
      field_settings,
      created_at: timestamp,
      modified_at: timestamp,
      deps_fields,
      deps_field_setting,
    };
    database_body
      .create_linked_view(&mut txn, params, field_orders, row_orders)
      .map_err(|err| {
        AppError::Internal(anyhow!(
          "Unable to create linked view for database view {}: {}",
          database_view_id,
          err
        ))
      })?;
    txn.encode_update_v1()
  };
  let collab_origin = GetCollabOrigin::User { uid };
  let (workspace_database_id, mut workspace_database) =
    get_latest_workspace_database(collab_storage, pg_pool, collab_origin.clone(), workspace_id)
      .await?;
  let workspace_database_update = add_new_database_view_for_workspace_database(
    &mut workspace_database,
    &database_id.to_string(),
    &new_view_id,
  )
  .await?;
  let mut folder =
    get_latest_collab_folder(collab_storage, GetCollabOrigin::User { uid }, workspace_id, client_id).await?;
  let folder_update = add_new_view_to_folder(
    uid,
    database_view_id,
    &new_view_id,
    &mut folder,
    name,
    to_folder_view_layout(view_layout.clone()),
  )
  .await?;
  update_database_data(
    appflowy_web_metrics,
    server.clone(),
    user.clone(),
    workspace_id,
    database_id,
    database_encoded_update,
  )
  .await?;
  update_workspace_database_data(
    appflowy_web_metrics,
    server.clone(),
    user.clone(),
    workspace_id,
    workspace_database_id,
    workspace_database_update,
  )
  .await?;
  update_workspace_folder_data(
    appflowy_web_metrics,
    server,
    user,
    workspace_id,
    folder_update,
  )
  .await?;

  Ok(())
}

#[instrument(level = "debug", skip_all)]
pub async fn update_page_collab_data(
  appflowy_web_metrics: &AppFlowyWebMetrics,
  server: Data<RealtimeServerAddr>,
  user: RealtimeUser,
  workspace_id: Uuid,
  object_id: Uuid,
  collab_type: CollabType,
  doc_state: Vec<u8>,
) -> Result<(), AppError> {
  appflowy_web_metrics.record_update_size_bytes(doc_state.len());

  let message = ClientHttpUpdateMessage {
    user,
    workspace_id,
    object_id,
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

#[instrument(level = "debug", skip_all)]
pub async fn update_workspace_folder_data(
  appflowy_web_metrics: &AppFlowyWebMetrics,
  server: Data<RealtimeServerAddr>,
  user: RealtimeUser,
  workspace_id: Uuid,
  update: Vec<u8>,
) -> Result<(), AppError> {
  appflowy_web_metrics.record_update_size_bytes(update.len());

  let (tx, rx) = tokio::sync::oneshot::channel();
  let message = ClientHttpUpdateMessage {
    user,
    workspace_id,
    object_id: workspace_id,
    collab_type: CollabType::Folder,
    update: Bytes::from(update),
    state_vector: None,
    return_tx: Some(tx),
  };

  server
    .try_send(message)
    .map_err(|err| AppError::Internal(anyhow!("Failed to send message to server: {}", err)))?;

  let resp = timeout_at(
    tokio::time::Instant::now() + Duration::from_millis(2000),
    rx,
  )
  .await
  .map_err(|err| {
    appflowy_web_metrics.incr_apply_update_timeout_count(1);
    AppError::Internal(anyhow!(
      "Failed to receive apply update within timeout: {}",
      err
    ))
  })?
  .map_err(|err| AppError::Internal(anyhow!("Unable to receive folder update reply: {}", err)))?;

  match resp {
    Ok(_) => Ok(()),
    Err(err) => {
      appflowy_web_metrics.incr_apply_update_failure_count(1);
      Err(AppError::Internal(anyhow!(
        "Failed to apply folder update: {}",
        err
      )))
    },
  }
}

#[instrument(level = "debug", skip_all)]
pub async fn update_workspace_database_data(
  appflowy_web_metrics: &AppFlowyWebMetrics,
  server: Data<RealtimeServerAddr>,
  user: RealtimeUser,
  workspace_id: Uuid,
  workspace_database_id: Uuid,
  update: Vec<u8>,
) -> Result<(), AppError> {
  appflowy_web_metrics.record_update_size_bytes(update.len());

  let (tx, rx) = tokio::sync::oneshot::channel();
  let message = ClientHttpUpdateMessage {
    user,
    workspace_id,
    object_id: workspace_database_id,
    collab_type: CollabType::WorkspaceDatabase,
    update: Bytes::from(update),
    state_vector: None,
    return_tx: Some(tx),
  };

  server
    .try_send(message)
    .map_err(|err| AppError::Internal(anyhow!("Failed to send message to server: {}", err)))?;

  let resp = timeout_at(
    tokio::time::Instant::now() + Duration::from_millis(2000),
    rx,
  )
  .await
  .map_err(|err| {
    appflowy_web_metrics.incr_apply_update_timeout_count(1);
    AppError::Internal(anyhow!(
      "Failed to receive apply update within timeout: {}",
      err
    ))
  })?
  .map_err(|err| {
    AppError::Internal(anyhow!(
      "Unable to receive workspace database update reply: {}",
      err
    ))
  })?;

  match resp {
    Ok(_) => Ok(()),
    Err(err) => {
      appflowy_web_metrics.incr_apply_update_failure_count(1);
      Err(AppError::Internal(anyhow!(
        "Failed to apply workspace database update: {}",
        err
      )))
    },
  }
}

#[instrument(level = "debug", skip_all)]
pub async fn update_database_data(
  appflowy_web_metrics: &AppFlowyWebMetrics,
  server: Data<RealtimeServerAddr>,
  user: RealtimeUser,
  workspace_id: Uuid,
  database_id: Uuid,
  update: Vec<u8>,
) -> Result<(), AppError> {
  appflowy_web_metrics.record_update_size_bytes(update.len());

  let (tx, rx) = tokio::sync::oneshot::channel();
  let message = ClientHttpUpdateMessage {
    user,
    workspace_id,
    object_id: database_id,
    collab_type: CollabType::Database,
    update: Bytes::from(update),
    state_vector: None,
    return_tx: Some(tx),
  };

  server
    .try_send(message)
    .map_err(|err| AppError::Internal(anyhow!("Failed to send message to server: {}", err)))?;

  let resp = timeout_at(
    tokio::time::Instant::now() + Duration::from_millis(2000),
    rx,
  )
  .await
  .map_err(|err| {
    appflowy_web_metrics.incr_apply_update_timeout_count(1);
    AppError::Internal(anyhow!(
      "Failed to receive apply update within timeout: {}",
      err
    ))
  })?
  .map_err(|err| AppError::Internal(anyhow!("Unable to receive database update reply: {}", err)))?;

  match resp {
    Ok(_) => Ok(()),
    Err(err) => {
      appflowy_web_metrics.incr_apply_update_failure_count(1);
      Err(AppError::Internal(anyhow!(
        "Failed to apply database update: {}",
        err
      )))
    },
  }
}
