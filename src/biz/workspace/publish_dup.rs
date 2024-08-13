use app_error::AppError;
use appflowy_collaborate::collab::storage::CollabAccessControlStorage;
use collab::core::collab::DataSource;
use collab::preclude::Collab;
use collab_database::database::Database;
use collab_database::rows::DatabaseRow;
use collab_database::views::ViewMap;
use collab_database::workspace_database::{DatabaseMetaList, WorkspaceDatabase};
use collab_document::blocks::DocumentData;
use collab_document::document::Document;
use collab_entity::CollabType;
use collab_folder::{CollabOrigin, Folder, RepeatedViewIdentifier, View, ViewLayout};
use collab_rt_entity::user::RealtimeUser;
use collab_rt_entity::{ClientCollabMessage, UpdateSync};
use collab_rt_protocol::{Message, SyncMessage};
use database::collab::{select_workspace_database_oid, CollabStorage};
use database::publish::select_published_data_for_view_id;
use database_entity::dto::CollabParams;
use shared_entity::dto::publish_dto::{PublishDatabaseData, PublishViewInfo, PublishViewMetaData};
use sqlx::PgPool;
use std::collections::HashSet;
use std::{collections::HashMap, sync::Arc};
use yrs::updates::encoder::Encode;

use crate::biz::collab::ops::get_latest_collab_encoded;
use crate::state::AppStateGroupManager;

#[allow(clippy::too_many_arguments)]
pub async fn duplicate_published_collab_to_workspace(
  pg_pool: &PgPool,
  collab_storage: Arc<CollabAccessControlStorage>,
  group_manager: AppStateGroupManager,
  dest_uid: i64,
  publish_view_id: String,
  dest_workspace_id: String,
  dest_view_id: String,
) -> Result<(), AppError> {
  let copier = PublishCollabDuplicator::new(
    pg_pool.clone(),
    collab_storage.clone(),
    group_manager,
    dest_uid,
    dest_workspace_id,
    dest_view_id,
  );
  copier.duplicate(&publish_view_id).await?;
  Ok(())
}

pub struct PublishCollabDuplicator {
  /// for fetching and writing folder data
  /// of dest workspace
  collab_storage: Arc<CollabAccessControlStorage>,
  /// A map to store the old view_id that was duplicated and new view_id assigned.
  /// If value is none, it means the view_id is not published.
  duplicated_refs: HashMap<String, Option<String>>,
  /// in case there's existing group, which contains the most updated collab data
  group_manager: AppStateGroupManager,
  /// A list of new views to be added to the folder
  views_to_add: Vec<View>,
  /// A list of database linked views to be added to workspace database
  workspace_databases: HashMap<String, Vec<String>>,
  /// time of duplication
  ts_now: i64,
  /// for fetching published data
  /// and writing them to dest workspace
  pg_pool: PgPool,
  /// user initiating the duplication
  duplicator_uid: i64,
  /// workspace to duplicate into
  dest_workspace_id: String,
  /// view of workspace to duplicate into
  dest_view_id: String,
}

impl PublishCollabDuplicator {
  pub fn new(
    pg_pool: PgPool,
    collab_storage: Arc<CollabAccessControlStorage>,
    group_manager: AppStateGroupManager,
    dest_uid: i64,
    dest_workspace_id: String,
    dest_view_id: String,
  ) -> Self {
    let ts_now = chrono::Utc::now().timestamp();
    Self {
      ts_now,
      duplicated_refs: HashMap::new(),
      views_to_add: Vec::new(),
      workspace_databases: HashMap::new(),

      pg_pool,
      collab_storage,
      group_manager,
      duplicator_uid: dest_uid,
      dest_workspace_id,
      dest_view_id,
    }
  }

  async fn duplicate(mut self, publish_view_id: &str) -> Result<(), AppError> {
    let mut txn = self.pg_pool.begin().await?;

    // new view after deep copy
    // this is the root of the document/database duplicated
    let mut root_view = match self
      .deep_copy(&mut txn, uuid::Uuid::new_v4().to_string(), publish_view_id)
      .await?
    {
      Some(v) => v,
      None => {
        txn.rollback().await?;
        return Err(AppError::RecordNotFound(
          "view not found, it might be unpublished".to_string(),
        ));
      },
    };
    //root_view.parent_view_id = self.dest_view_id.clone();
    root_view.parent_view_id.clone_from(&self.dest_view_id);

    // update database if any
    if !self.workspace_databases.is_empty() {
      let ws_db_oid =
        select_workspace_database_oid(&self.pg_pool, &self.dest_workspace_id.parse()?).await?;
      let ws_db_collab = {
        let ws_database_ec = get_latest_collab_encoded(
          self.group_manager.clone(),
          self.collab_storage.clone(),
          &self.duplicator_uid,
          &self.dest_workspace_id,
          &ws_db_oid,
          CollabType::WorkspaceDatabase,
        )
        .await?;
        collab_from_doc_state(ws_database_ec.doc_state.to_vec(), &ws_db_oid)?
      };

      let ws_db_meta_list = DatabaseMetaList::from_collab(&ws_db_collab);
      let ws_db_updates = {
        let mut txn_wrapper = ws_db_collab.origin_transact_mut();
        for (db_collab_id, linked_views) in &self.workspace_databases {
          ws_db_meta_list.add_database_with_txn(
            &mut txn_wrapper,
            db_collab_id,
            linked_views.clone(),
          );
        }
        txn_wrapper.encode_update_v1()
      };
      self.broadcast_update(&ws_db_oid, ws_db_updates).await;
      let updated_ws_w_db_collab = ws_db_collab
        .encode_collab_v1(WorkspaceDatabase::validate)
        .map_err(|e| AppError::Unhandled(e.to_string()))?;
      self
        .insert_collab_for_duplicator(
          &ws_db_collab.object_id,
          updated_ws_w_db_collab.encode_to_bytes()?,
          CollabType::WorkspaceDatabase,
          &mut txn,
        )
        .await?;
    }

    let collab_folder_encoded = get_latest_collab_encoded(
      self.group_manager.clone(),
      self.collab_storage.clone(),
      &self.duplicator_uid,
      &self.dest_workspace_id,
      &self.dest_workspace_id,
      CollabType::Folder,
    )
    .await?;

    let folder = Folder::from_collab_doc_state(
      self.duplicator_uid,
      CollabOrigin::Server,
      DataSource::DocStateV1(collab_folder_encoded.doc_state.to_vec()),
      &self.dest_workspace_id,
      vec![],
    )
    .map_err(|e| AppError::Unhandled(e.to_string()))?;

    let encoded_update = folder.get_updates_for_op(|folder| {
      // add all views required to the folder
      folder.insert_view(root_view, None);
      for view in &self.views_to_add {
        folder.insert_view(view.clone(), None);
      }
    });

    // update folder collab
    let updated_encoded_collab = folder
      .encode_collab_v1()
      .map_err(|e| AppError::Unhandled(e.to_string()))?;

    // insert updated folder collab
    self
      .insert_collab_for_duplicator(
        &self.dest_workspace_id.clone(),
        updated_encoded_collab.encode_to_bytes()?,
        CollabType::Folder,
        &mut txn,
      )
      .await?;

    // broadcast folder changes
    self
      .broadcast_update(&self.dest_workspace_id, encoded_update)
      .await;

    txn.commit().await?;
    Ok(())
  }

  /// Deep copy a published collab to the destination workspace.
  /// If None is returned, it means the view is not published.
  /// If Some is returned, a new view is created but without parent_view_id set.
  /// Caller should set the parent_view_id to the parent view.
  async fn deep_copy(
    &mut self,
    txn: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    new_view_id: String,
    publish_view_id: &str,
  ) -> Result<Option<View>, AppError> {
    tracing::info!(
      "deep_copy: new_view_id: {}, publish_view_id: {}",
      new_view_id,
      publish_view_id,
    );

    // attempt to get metadata and doc_state for published view
    let (metadata, published_blob) =
      match get_published_data_for_view_id(txn, &publish_view_id.parse()?).await? {
        Some(published_data) => published_data,
        None => {
          tracing::warn!(
            "No published collab data found for view_id: {}",
            publish_view_id
          );
          return Ok(None);
        },
      };

    // at this stage, we know that the view is published,
    // so we insert this knowledge into the duplicated_refs
    self
      .duplicated_refs
      .insert(publish_view_id.to_string(), new_view_id.clone().into());

    match metadata.view.layout {
      ViewLayout::Document => {
        let doc = Document::from_doc_state(
          CollabOrigin::Empty,
          DataSource::DocStateV1(published_blob.to_vec()),
          "",
          vec![],
        )
        .map_err(|e| AppError::Unhandled(e.to_string()))?;

        let new_doc_view = self.deep_copy_doc(txn, new_view_id, doc, metadata).await?;
        Ok(Some(new_doc_view))
      },
      ViewLayout::Grid | ViewLayout::Board | ViewLayout::Calendar => {
        let db_payload = serde_json::from_slice::<PublishDatabaseData>(&published_blob)?;
        let new_db_view = self
          .deep_copy_database(
            txn,
            new_view_id,
            uuid::Uuid::new_v4().to_string(),
            db_payload,
            metadata,
          )
          .await?;
        Ok(Some(new_db_view))
      },
      t => {
        tracing::warn!("collab type not supported: {:?}", t);
        Ok(None)
      },
    }
  }

  async fn deep_copy_doc<'a>(
    &mut self,
    txn: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    new_view_id: String,
    doc: Document,
    metadata: PublishViewMetaData,
  ) -> Result<View, AppError> {
    let mut ret_view =
      self.new_folder_view(new_view_id.clone(), &metadata.view, ViewLayout::Document);

    let mut doc_data = doc
      .get_document_data()
      .map_err(|e| AppError::Unhandled(e.to_string()))?;

    if let Err(err) = self
      .deep_copy_doc_pages(txn, &mut doc_data, &mut ret_view)
      .await
    {
      tracing::error!("failed to deep copy doc pages: {}", err);
    }

    if let Err(err) = self
      .deep_copy_doc_databases(txn, &mut doc_data, &mut ret_view)
      .await
    {
      tracing::error!("failed to deep copy doc databases: {}", err);
    };

    // doc_data into binary data
    let new_doc_data = {
      let collab = doc.get_collab().clone();
      let new_doc = Document::create_with_data(collab, doc_data)
        .map_err(|e| AppError::Unhandled(e.to_string()))?;
      let encoded_collab = new_doc
        .encode_collab()
        .map_err(|e| AppError::Unhandled(e.to_string()))?;
      encoded_collab.encode_to_bytes()?
    };

    // insert document with modified page_id references
    self
      .insert_collab_for_duplicator(&ret_view.id, new_doc_data, CollabType::Document, txn)
      .await?;

    Ok(ret_view)
  }

  async fn deep_copy_doc_pages(
    &mut self,
    txn: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    doc_data: &mut DocumentData,
    ret_view: &mut View,
  ) -> Result<(), AppError> {
    let page_ids = doc_data
      .blocks
      .values_mut()
      .flat_map(|block| block.data.iter_mut())
      .filter(|(key, _)| *key == "delta")
      .flat_map(|(_, value)| value.as_array_mut())
      .flatten()
      .flat_map(|delta| delta.get_mut("attributes"))
      .flat_map(|attributes| attributes.get_mut("mention"))
      .filter(|mention| {
        mention.get("type").map_or(false, |type_| {
          type_.as_str().map_or(false, |type_| type_ == "page")
        })
      })
      .flat_map(|mention| mention.get_mut("page_id"));

    // deep copy all the page_id references
    for page_id in page_ids {
      let page_id_str = match page_id.as_str() {
        Some(page_id_str) => page_id_str,
        None => continue,
      };
      match self.duplicated_refs.get(page_id_str) {
        Some(new_view_id) => {
          if let Some(vid) = new_view_id {
            *page_id = serde_json::json!(vid);
          } else {
            // ref view_id is not published
            // TODO: handle this case to
            // display better in the UI?
          }
        },
        None => {
          // Call deep_copy and await the result
          if let Some(mut new_view) =
            Box::pin(self.deep_copy(txn, uuid::Uuid::new_v4().to_string(), page_id_str)).await?
          {
            new_view.parent_view_id.clone_from(&ret_view.id);
            self
              .duplicated_refs
              .insert(page_id_str.to_string(), Some(new_view.id.clone()));
            self.views_to_add.push(new_view.clone());
            *page_id = serde_json::json!(new_view.id);
          } else {
            self.duplicated_refs.insert(page_id_str.to_string(), None);
          }
        },
      }
    }

    // update text map
    if let Some(text_map) = doc_data.meta.text_map.as_mut() {
      for (_key, value) in text_map.iter_mut() {
        let mut js_val = match serde_json::from_str::<serde_json::Value>(value) {
          Ok(js_val) => js_val,
          Err(e) => {
            tracing::error!("failed to parse text_map value({}): {}", value, e);
            continue;
          },
        };
        let js_array = match js_val.as_array_mut() {
          Some(js_array) => js_array,
          None => continue,
        };
        js_array
          .iter_mut()
          .flat_map(|js_val| js_val.get_mut("attributes"))
          .flat_map(|attributes| attributes.get_mut("mention"))
          .filter(|mention| {
            mention.get("type").map_or(false, |type_| {
              type_.as_str().map_or(false, |type_| type_ == "page")
            })
          })
          .flat_map(|mention| mention.get_mut("page_id"))
          .for_each(|page_id| {
            let page_id_str = match page_id.as_str() {
              Some(page_id_str) => page_id_str,
              None => return,
            };
            if let Some(new_page_id) = self.duplicated_refs.get(page_id_str) {
              *page_id = serde_json::json!(new_page_id);
            }
          });
        *value = js_val.to_string();
      }
    }

    Ok(())
  }

  async fn deep_copy_doc_databases(
    &mut self,
    txn: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    doc_data: &mut DocumentData,
    ret_view: &mut View,
  ) -> Result<(), AppError> {
    let db_blocks = doc_data
      .blocks
      .iter_mut()
      .filter(|(_, b)| b.ty == "grid" || b.ty == "board" || b.ty == "calendar");

    for (_block_id, block) in db_blocks {
      let block_view_id = block
        .data
        .get_mut("view_id")
        .ok_or_else(|| AppError::RecordNotFound("view_id not found in block data".to_string()))?;
      let view_id_str = block_view_id
        .as_str()
        .ok_or_else(|| AppError::RecordNotFound("view_id not a string".to_string()))?;

      if let Some(mut new_folder_db_view) =
        Box::pin(self.deep_copy(txn, view_id_str.to_string(), view_id_str)).await?
      {
        new_folder_db_view.parent_view_id.clone_from(&ret_view.id);
        self.views_to_add.push(new_folder_db_view);

        // update view_id
        if let Some(id) = self.old_to_new_view_id(view_id_str) {
          *block_view_id = serde_json::Value::String(id);
        } else {
          tracing::warn!("view_id not found in old_to_new_view_id: {}", view_id_str);
        }

        // update parent_id
        let block_parent_id = block.data.get_mut("parent_id").ok_or_else(|| {
          AppError::RecordNotFound("parent_id not found in block data".to_string())
        })?;
        let parent_view_id_str = block_parent_id
          .as_str()
          .ok_or_else(|| AppError::RecordNotFound("view_id not a string".to_string()))?;
        if let Some(new_id) = self.old_to_new_view_id(parent_view_id_str) {
          *block_parent_id = serde_json::Value::String(new_id);
        } else {
          tracing::warn!(
            "view_id not found in old_to_new_view_id: {}",
            parent_view_id_str
          );
        }
      }
    }

    Ok(())
  }

  /// Deep copy a published database to the destination workspace.
  /// Returns the Folder view for main view (`new_view_id`) and map from old to new view_id
  async fn deep_copy_database<'a>(
    &mut self,
    pg_txn: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    new_view_id: String,
    new_db_id: String,
    published_db: PublishDatabaseData,
    metadata: PublishViewMetaData,
  ) -> Result<View, AppError> {
    let pub_view_id = metadata.view.view_id.as_str();

    // flatten nested view info into a map
    let view_info_by_id = view_info_by_view_id(&metadata);

    // collab of database
    let db_collab = collab_from_doc_state(published_db.database_collab.clone(), &new_db_id)?;

    // duplicate collab rows
    // key: old_row_id -> Collab (with new_id and database_id)
    for (old_id, v) in &published_db.database_row_collabs {
      // assign a new id for the row
      let new_row_id = uuid::Uuid::new_v4().to_string();

      let db_row_collab = collab_from_doc_state(v.clone(), &new_row_id)?;

      db_row_collab.with_origin_transact_mut(|txn| {
        if let Some(container) = db_row_collab.get_map_with_txn(txn, vec!["data"]) {
          // TODO(Zack): deep copy row data ?
          container.insert_with_txn(txn, "id", new_row_id.clone());
          container.insert_with_txn(txn, "database_id", new_db_id.clone());
        }
      });

      let db_row_ec_bytes = db_row_collab
        .encode_collab_v1(DatabaseRow::validate)
        .map_err(|e| AppError::Unhandled(e.to_string()))?
        .encode_to_bytes()?;
      self
        .insert_collab_for_duplicator(
          &new_row_id,
          db_row_ec_bytes,
          CollabType::DatabaseRow,
          pg_txn,
        )
        .await?;

      self
        .duplicated_refs
        .insert(old_id.clone(), Some(new_row_id));
    }

    // create a new view to be returned to the caller
    // view_id is the main view of the database
    let ret_view = {
      // create a txn that will be drop at the end of the block
      let mut txn = db_collab.origin_transact_mut();

      if let Some(container) = db_collab.get_map_with_txn(txn.txn(), vec!["database", "fields"]) {
        container.insert_with_txn(&mut txn, "id", new_db_id.clone());
      }
      if let Some(container) = db_collab.get_map_with_txn(txn.txn(), vec!["database"]) {
        container.insert_with_txn(&mut txn, "id", new_db_id.clone());
      }

      // accumulate list of database views (Board, Cal, ...) to be linked to the database
      let mut new_db_view_ids: Vec<String> = vec![];

      // Set the row_id references
      let view_change_tx = tokio::sync::broadcast::channel(1).0;
      let view_map = {
        let container = db_collab
          .get_map_with_txn(txn.txn(), vec!["database", "views"])
          .ok_or_else(|| AppError::RecordNotFound("no views found in database".to_string()))?;
        ViewMap::new(container, view_change_tx)
      };
      let visible_database_view_ids = published_db
        .visible_database_view_ids
        .iter()
        .map(|v| v.as_str())
        .collect::<HashSet<_>>();

      let mut db_views = view_map
        .get_all_views_with_txn(txn.txn())
        .into_iter()
        .filter(|view| {
          view.id == pub_view_id || visible_database_view_ids.contains(view.id.as_str())
        })
        .collect::<Vec<_>>();
      if db_views.is_empty() {
        return Err(AppError::RecordNotFound(
          "no (visible) views found in database".to_string(),
        ));
      }

      let selected_view = {
        let mut selected_view: Option<View> = None;
        // rest of the views are child of main db view
        for db_view in db_views.iter_mut() {
          db_view.database_id.clone_from(&new_db_id);
          let pub_view_info = match view_info_by_id.get(&db_view.id) {
            Some(view_info) => view_info,
            None => {
              // skip if view not found
              tracing::warn!("metadata not found for view: {}", db_view.id);
              continue;
            },
          };
          if pub_view_id == db_view.id.as_str() {
            // main view that is duplicated
            db_view.id.clone_from(&new_view_id);
            new_db_view_ids.push(new_view_id.clone());
            selected_view = Some(self.new_folder_view(
              new_view_id.clone(),
              pub_view_info,
              db_layout_to_view_layout(db_view.layout),
            ));
          } else {
            let other_view_id = uuid::Uuid::new_v4().to_string();
            self
              .duplicated_refs
              .insert(db_view.id.clone(), Some(other_view_id.clone()));
            new_db_view_ids.push(other_view_id.clone());
            db_view.id.clone_from(&other_view_id);
            let mut other_folder_view = self.new_folder_view(
              other_view_id,
              pub_view_info,
              db_layout_to_view_layout(db_view.layout),
            );
            other_folder_view.parent_view_id.clone_from(&new_view_id);
            self.views_to_add.push(other_folder_view);
          }
        }
        selected_view.ok_or_else(|| AppError::RecordNotFound("main view not found".to_string()))?
      };

      // update all views's row's id
      for db_view in db_views.iter_mut() {
        for row_order in db_view.row_orders.iter_mut() {
          if let Some(new_id) = self.old_to_new_view_id(row_order.id.as_str()) {
            row_order.id = new_id.into();
          } else {
            // skip if row not found
            tracing::warn!("row not found: {}", row_order.id);
            continue;
          }
        }
      }

      // insert updated views back to db
      view_map.clear_with_txn(&mut txn);
      for view in db_views {
        view_map.insert_view_with_txn(&mut txn, view);
      }

      // Add this database as linked view
      self
        .workspace_databases
        .insert(new_db_id.clone(), new_db_view_ids);

      selected_view
    };

    // insert database with modified row_id references
    let db_encoded_collab = db_collab
      .encode_collab_v1(Database::validate)
      .map_err(|e| AppError::Unhandled(e.to_string()))?
      .encode_to_bytes()?;
    self
      .insert_collab_for_duplicator(&new_db_id, db_encoded_collab, CollabType::Database, pg_txn)
      .await?;

    Ok(ret_view)
  }

  /// ceates a new folder view without parent_view_id set
  fn new_folder_view(
    &self,
    new_view_id: String,
    view_info: &PublishViewInfo,
    layout: ViewLayout,
  ) -> View {
    View {
      id: new_view_id.clone(),
      parent_view_id: "".to_string(), // to be filled by caller
      name: view_info.name.clone(),
      desc: "".to_string(), // unable to get from metadata
      children: RepeatedViewIdentifier { items: vec![] }, // fill in while iterating children
      created_at: self.ts_now,
      is_favorite: false,
      layout,
      icon: view_info.icon.clone(),
      created_by: Some(self.duplicator_uid),
      last_edited_time: self.ts_now,
      last_edited_by: Some(self.duplicator_uid),
      extra: view_info.extra.clone(),
    }
  }

  async fn insert_collab_for_duplicator(
    &self,
    oid: &str,
    encoded_collab: Vec<u8>,
    collab_type: CollabType,
    txn: &mut sqlx::Transaction<'_, sqlx::Postgres>,
  ) -> Result<(), AppError> {
    tracing::info!(
      "inserting collab for duplicator: {} {} {}",
      oid,
      collab_type,
      encoded_collab.len()
    );
    self
      .collab_storage
      .insert_new_collab_with_transaction(
        &self.dest_workspace_id,
        &self.duplicator_uid,
        CollabParams {
          object_id: oid.to_string(),
          encoded_collab_v1: encoded_collab,
          collab_type,
          embeddings: None,
        },
        txn,
      )
      .await?;
    Ok(())
  }

  /// broadcast updates to collab group if exists
  async fn broadcast_update(&self, oid: &str, encoded_update: Vec<u8>) {
    tracing::info!("broadcasting update to group: {}", oid);
    match self.group_manager.get_group(oid).await {
      Some(group) => {
        let (collab_message_sender, _collab_message_receiver) = futures::channel::mpsc::channel(1);
        let (mut message_by_oid_sender, message_by_oid_receiver) =
          futures::channel::mpsc::channel(1);
        group
          .subscribe(
            &RealtimeUser {
              uid: self.duplicator_uid,
              device_id: uuid::Uuid::new_v4().to_string(),
              connect_at: self.ts_now,
              session_id: uuid::Uuid::new_v4().to_string(),
              app_version: "".to_string(),
            },
            CollabOrigin::Server,
            collab_message_sender,
            message_by_oid_receiver,
          )
          .await;
        let payload = Message::Sync(SyncMessage::Update(encoded_update)).encode_v1();
        let message = HashMap::from([(
          oid.to_string(),
          vec![ClientCollabMessage::ClientUpdateSync {
            data: UpdateSync {
              origin: CollabOrigin::Server,
              object_id: oid.to_string(),
              msg_id: self.ts_now as u64,
              payload: payload.into(),
            },
          }],
        )]);
        match message_by_oid_sender.try_send(message) {
          Ok(()) => tracing::info!("sent message to group"),
          Err(err) => tracing::error!("failed to send message to group: {}", err),
        }
      },
      None => tracing::warn!("group not found for oid: {}", oid),
    }
  }

  fn old_to_new_view_id(&self, old_view_id: &str) -> Option<String> {
    self.duplicated_refs.get(old_view_id).cloned().flatten()
  }
}

fn db_layout_to_view_layout(layout: collab_database::views::DatabaseLayout) -> ViewLayout {
  match layout {
    collab_database::views::DatabaseLayout::Grid => ViewLayout::Grid,
    collab_database::views::DatabaseLayout::Board => ViewLayout::Board,
    collab_database::views::DatabaseLayout::Calendar => ViewLayout::Calendar,
  }
}

async fn get_published_data_for_view_id(
  txn: &mut sqlx::Transaction<'_, sqlx::Postgres>,
  view_id: &uuid::Uuid,
) -> Result<Option<(PublishViewMetaData, Vec<u8>)>, AppError> {
  match select_published_data_for_view_id(txn, view_id).await? {
    Some((js_val, blob)) => {
      let metadata = serde_json::from_value(js_val)?;
      Ok(Some((metadata, blob)))
    },
    None => Ok(None),
  }
}

fn view_info_by_view_id(meta: &PublishViewMetaData) -> HashMap<String, PublishViewInfo> {
  let mut acc = HashMap::new();
  acc.insert(meta.view.view_id.clone(), meta.view.clone());
  view_info_map(&mut acc, &meta.child_views);
  acc
}

fn view_info_map(acc: &mut HashMap<String, PublishViewInfo>, view_infos: &[PublishViewInfo]) {
  for view_info in view_infos {
    acc.insert(view_info.view_id.clone(), view_info.clone());
    if let Some(child_views) = &view_info.child_views {
      view_info_map(acc, child_views);
    }
  }
}

fn collab_from_doc_state(doc_state: Vec<u8>, object_id: &str) -> Result<Collab, AppError> {
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
