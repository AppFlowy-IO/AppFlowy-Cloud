use app_error::AppError;
use appflowy_collaborate::collab::storage::CollabAccessControlStorage;
use collab::core::collab::DataSource;
use collab::preclude::Collab;

use collab::preclude::MapExt;
use collab_database::views::ViewMap;
use collab_database::workspace_database::DatabaseMetaList;
use collab_document::blocks::DocumentData;
use collab_document::document::Document;
use collab_entity::CollabType;
use collab_folder::{CollabOrigin, Folder, RepeatedViewIdentifier, View};
use collab_rt_entity::{ClientCollabMessage, UpdateSync};
use collab_rt_protocol::{Message, SyncMessage};
use database::collab::{select_workspace_database_oid, CollabStorage};
use database::publish::select_published_data_for_view_id;
use database_entity::dto::CollabParams;
use shared_entity::dto::publish_dto::{PublishDatabaseData, PublishViewInfo, PublishViewMetaData};
use shared_entity::dto::workspace_dto;
use shared_entity::dto::workspace_dto::ViewLayout;
use sqlx::PgPool;
use std::collections::HashSet;
use std::{collections::HashMap, sync::Arc};
use yrs::updates::encoder::Encode;
use yrs::{Map, MapRef};

use crate::biz::collab::ops::get_latest_collab_encoded;

#[allow(clippy::too_many_arguments)]
pub async fn duplicate_published_collab_to_workspace(
  pg_pool: &PgPool,
  collab_storage: Arc<CollabAccessControlStorage>,
  dest_uid: i64,
  publish_view_id: String,
  dest_workspace_id: String,
  dest_view_id: String,
) -> Result<(), AppError> {
  let copier = PublishCollabDuplicator::new(
    pg_pool.clone(),
    collab_storage.clone(),
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
  /// published_database_id -> view_id
  duplicated_db_main_view: HashMap<String, String>,
  /// published_database_view_id -> new_view_id
  duplicated_db_view: HashMap<String, String>,
  /// new views to be added to the folder
  /// view_id -> view
  views_to_add: HashMap<String, View>,
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
    dest_uid: i64,
    dest_workspace_id: String,
    dest_view_id: String,
  ) -> Self {
    let ts_now = chrono::Utc::now().timestamp();
    Self {
      ts_now,
      duplicated_refs: HashMap::new(),
      views_to_add: HashMap::new(),
      workspace_databases: HashMap::new(),
      duplicated_db_main_view: HashMap::new(),
      duplicated_db_view: HashMap::new(),

      pg_pool,
      collab_storage,
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
      let mut ws_db_collab = {
        let ws_database_ec = get_latest_collab_encoded(
          self.collab_storage.clone(),
          &self.duplicator_uid,
          &self.dest_workspace_id,
          &ws_db_oid,
          CollabType::WorkspaceDatabase,
        )
        .await?;
        collab_from_doc_state(ws_database_ec.doc_state.to_vec(), &ws_db_oid)?
      };

      let ws_db_meta_list = DatabaseMetaList::new(&mut ws_db_collab);
      let ws_db_updates = {
        let mut txn_wrapper = ws_db_collab.transact_mut();
        for (db_collab_id, linked_views) in &self.workspace_databases {
          ws_db_meta_list.add_database(&mut txn_wrapper, db_collab_id, linked_views.clone());
        }
        txn_wrapper.encode_update_v1()
      };
      self.broadcast_update(&ws_db_oid, ws_db_updates).await?;
      let updated_ws_w_db_collab = ws_db_collab
        .encode_collab_v1(|collab| CollabType::WorkspaceDatabase.validate_require_data(collab))
        .map_err(|e| AppError::Unhandled(e.to_string()))?;

      self
        .insert_collab_for_duplicator(
          ws_db_collab.object_id(),
          updated_ws_w_db_collab.encode_to_bytes()?,
          CollabType::WorkspaceDatabase,
          &mut txn,
        )
        .await?;
    }

    let collab_folder_encoded = get_latest_collab_encoded(
      self.collab_storage.clone(),
      &self.duplicator_uid,
      &self.dest_workspace_id,
      &self.dest_workspace_id,
      CollabType::Folder,
    )
    .await?;

    let mut folder = Folder::from_collab_doc_state(
      self.duplicator_uid,
      CollabOrigin::Server,
      collab_folder_encoded.into(),
      &self.dest_workspace_id,
      vec![],
    )
    .map_err(|e| AppError::Unhandled(e.to_string()))?;

    let encoded_update = {
      let mut folder_txn = folder.collab.transact_mut();

      let mut duplicated_view_ids = HashSet::new();
      duplicated_view_ids.insert(root_view.id.clone());
      folder.body.views.insert(&mut folder_txn, root_view, None);

      // when child views are added, it must have a parent view that is previously added
      // TODO: if there are too many child views, consider using topological sort
      loop {
        if self.views_to_add.is_empty() {
          break;
        }

        let mut inserted = vec![];
        for (view_id, view) in self.views_to_add.iter() {
          if duplicated_view_ids.contains(&view.parent_view_id) {
            folder
              .body
              .views
              .insert(&mut folder_txn, view.clone(), None);
            duplicated_view_ids.insert(view_id.clone());
            inserted.push(view_id.clone());
          }
        }
        if inserted.is_empty() {
          tracing::error!(
            "views not inserted because parent_id does not exists: {:?}",
            self.views_to_add.keys()
          );
          break;
        }
        for view_id in inserted {
          self.views_to_add.remove(&view_id);
        }
      }

      folder_txn.encode_update_v1()
    };

    // update folder collab
    let updated_encoded_collab = folder
      .encode_collab_v1(|collab| CollabType::Folder.validate_require_data(collab))
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
      .await?;

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
        let doc_collab = collab_from_doc_state(published_blob, "")?;
        let doc = Document::open(doc_collab).map_err(|e| AppError::Unhandled(e.to_string()))?;
        let new_doc_view = self.deep_copy_doc(txn, new_view_id, doc, metadata).await?;
        Ok(Some(new_doc_view))
      },
      ViewLayout::Grid | ViewLayout::Board | ViewLayout::Calendar => {
        let pub_view_id = metadata.view.view_id.clone();
        let db_payload = serde_json::from_slice::<PublishDatabaseData>(&published_blob)?;
        let new_db_view = self
          .deep_copy_database_view(txn, new_view_id, db_payload, &metadata, &pub_view_id)
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

    {
      // write modified doc_data back to storage
      let empty_collab = collab_from_doc_state(vec![], &new_view_id)?;
      let new_doc = Document::open_with(empty_collab, Some(doc_data))
        .map_err(|e| AppError::Unhandled(e.to_string()))?;
      let new_doc_bin = new_doc
        .encode_collab()
        .map_err(|e| AppError::Unhandled(e.to_string()))?
        .encode_to_bytes()?;
      self
        .insert_collab_for_duplicator(&ret_view.id, new_doc_bin, CollabType::Document, txn)
        .await?;
    }

    Ok(ret_view)
  }

  async fn deep_copy_doc_pages(
    &mut self,
    txn: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    doc_data: &mut DocumentData,
    ret_view: &mut View,
  ) -> Result<(), AppError> {
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

        let page_ids = js_array
          .iter_mut()
          .flat_map(|js_val| js_val.get_mut("attributes"))
          .flat_map(|attributes| attributes.get_mut("mention"))
          .filter(|mention| {
            mention.get("type").map_or(false, |type_| {
              type_.as_str().map_or(false, |type_| type_ == "page")
            })
          })
          .flat_map(|mention| mention.get_mut("page_id"));

        for page_id in page_ids {
          let page_id_str = match page_id.as_str() {
            Some(page_id_str) => page_id_str,
            None => continue,
          };
          if let Some(new_page_id) = self.deep_copy_view(page_id_str, txn, &ret_view.id).await? {
            *page_id = serde_json::json!(new_page_id);
          } else {
            tracing::warn!("deep_copy_doc_pages: view not found: {}", page_id_str);
          };
        }

        *value = js_val.to_string();
      }
    }

    Ok(())
  }

  /// attempts to deep copy a view using `view_id`. returns a new_view_id of the duplicated view.
  /// if view is already duplicated, returns duplicated view's view_id (parent_view_id is not set
  /// from param `parent_view_id`)
  async fn deep_copy_view(
    &mut self,
    view_id: &str,
    txn: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    parent_view_id: &String,
  ) -> Result<Option<String>, AppError> {
    match self.duplicated_refs.get(view_id) {
      Some(new_view_id) => {
        if let Some(vid) = new_view_id {
          Ok(Some(vid.clone()))
        } else {
          Ok(None)
        }
      },
      None => {
        // Call deep_copy and await the result
        if let Some(mut new_view) =
          Box::pin(self.deep_copy(txn, uuid::Uuid::new_v4().to_string(), view_id)).await?
        {
          if new_view.parent_view_id.is_empty() {
            new_view.parent_view_id.clone_from(parent_view_id);
          }
          self
            .duplicated_refs
            .insert(view_id.to_string(), Some(new_view.id.clone()));
          let ret_view_id = new_view.id.clone();
          self.views_to_add.insert(new_view.id.clone(), new_view);
          Ok(Some(ret_view_id))
        } else {
          tracing::warn!("view not found in deep_copy: {}", view_id);
          self.duplicated_refs.insert(view_id.to_string(), None);
          Ok(None)
        }
      },
    }
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

    for (block_id, block) in db_blocks {
      tracing::info!("deep_copy_doc_databases: block_id: {}", block_id);
      let block_view_id = block
        .data
        .get("view_id")
        .ok_or_else(|| AppError::RecordNotFound("view_id not found in block data".to_string()))?
        .as_str()
        .ok_or_else(|| AppError::RecordNotFound("view_id not a string".to_string()))?;

      let block_parent_id = block
        .data
        .get("parent_id")
        .ok_or_else(|| AppError::RecordNotFound("view_id not found in block data".to_string()))?
        .as_str()
        .ok_or_else(|| AppError::RecordNotFound("view_id not a string".to_string()))?;

      if let Some((new_view_id, new_parent_id)) = self
        .deep_copy_database_inline_doc(txn, block_view_id, block_parent_id, &ret_view.id)
        .await?
      {
        block.data.insert(
          "view_id".to_string(),
          serde_json::Value::String(new_view_id),
        );
        block.data.insert(
          "parent_id".to_string(),
          serde_json::Value::String(new_parent_id),
        );
      } else {
        tracing::warn!("deep_copy_doc_databases: view not found: {}", block_view_id);
      }
    }

    Ok(())
  }

  /// deep copy database for doc
  /// returns new (view_id, parent_id)
  async fn deep_copy_database_inline_doc<'a>(
    &mut self,
    txn: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    view_id: &str,
    parent_id: &str,
    doc_view_id: &String,
  ) -> Result<Option<(String, String)>, AppError> {
    let (metadata, published_blob) =
      match get_published_data_for_view_id(txn, &view_id.parse()?).await? {
        Some(published_data) => published_data,
        None => {
          tracing::warn!("No published collab data found for view_id: {}", view_id);
          return Ok(None);
        },
      };

    let published_db = serde_json::from_slice::<PublishDatabaseData>(&published_blob)?;
    let mut parent_view = self
      .deep_copy_database_view(
        txn,
        uuid::Uuid::new_v4().to_string(),
        published_db,
        &metadata,
        parent_id,
      )
      .await?;
    let parent_view_id = parent_view.id.clone();
    if parent_view.parent_view_id.is_empty() {
      parent_view.parent_view_id.clone_from(doc_view_id);
      self
        .views_to_add
        .insert(parent_view.id.clone(), parent_view);
    }
    let duplicated_view_id = match self.duplicated_db_view.get(view_id) {
      Some(v) => v.clone(),
      None => {
        let view_info_by_id = view_info_by_view_id(&metadata);
        let view_info = view_info_by_id.get(view_id).ok_or_else(|| {
          AppError::RecordNotFound(format!("metadata not found for view: {}", view_id))
        })?;
        let mut new_folder_db_view =
          self.new_folder_view(view_id.to_string(), view_info, view_info.layout.clone());
        new_folder_db_view.parent_view_id = parent_view_id.clone();
        let new_folder_db_view_id = new_folder_db_view.id.clone();
        self
          .views_to_add
          .insert(new_folder_db_view.id.clone(), new_folder_db_view);
        new_folder_db_view_id
      },
    };
    Ok(Some((duplicated_view_id, parent_view_id)))
  }

  /// Deep copy a published database (does not create folder views)
  /// checks if database is already published
  /// attempts to use `new_view_id` for `published_view_id` if not already published
  /// stores all view_id references in `duplicated_refs`
  /// returns (published_db_id, new_db_id, is_already_duplicated)
  async fn deep_copy_database<'a>(
    &mut self,
    pg_txn: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    published_db: &PublishDatabaseData,
    publish_view_id: &str,
    new_view_id: String,
  ) -> Result<(String, String, bool), AppError> {
    // collab of database
    let mut db_collab = collab_from_doc_state(published_db.database_collab.clone(), "")?;
    let pub_db_id = get_database_id_from_collab(&db_collab)?;

    // check if the database is already duplicated
    if let Some(db_id) = self.duplicated_refs.get(&pub_db_id).cloned().flatten() {
      return Ok((pub_db_id, db_id, true));
    }

    let new_db_id = uuid::Uuid::new_v4().to_string();
    self
      .duplicated_refs
      .insert(pub_db_id.clone(), Some(new_db_id.clone()));

    // duplicate db collab rows
    for (old_id, row_bin_data) in &published_db.database_row_collabs {
      // assign a new id for the row
      let new_row_id = uuid::Uuid::new_v4().to_string();
      let mut db_row_collab = collab_from_doc_state(row_bin_data.clone(), &new_row_id)?;

      {
        // update database_id and row_id in data
        let mut txn = db_row_collab.context.transact_mut();
        let data = db_row_collab
          .data
          .get(&txn, "data")
          .ok_or_else(|| {
            AppError::RecordNotFound("no data found in database row collab".to_string())
          })?
          .cast::<MapRef>()
          .map_err(|err| AppError::Unhandled(format!("data not map: {:?}", err)))?;
        data.insert(&mut txn, "id", new_row_id.clone());
        data.insert(&mut txn, "database_id", new_db_id.clone());
      }

      // write new row collab to storage
      let db_row_ec_bytes = db_row_collab
        .encode_collab_v1(|collab| CollabType::DatabaseRow.validate_require_data(collab))
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

    // accumulate list of database views (Board, Cal, ...) to be linked to the database
    let mut new_db_view_ids: Vec<String> = vec![];
    {
      let mut txn = db_collab.context.transact_mut();
      let container = db_collab
        .data
        .get(&txn, "database")
        .ok_or_else(|| AppError::RecordNotFound("no database found in collab".to_string()))?
        .cast::<MapRef>()
        .map_err(|err| AppError::Unhandled(format!("not a map: {:?}", err)))?;
      container.insert(&mut txn, "id", new_db_id.clone());

      let view_map = {
        let map_ref = db_collab
          .data
          .get_with_path(&txn, ["database", "views"])
          .ok_or_else(|| AppError::RecordNotFound("no views found in database".to_string()))?;
        ViewMap::new(map_ref, tokio::sync::broadcast::channel(1).0)
      };

      // create new database views based on published views
      let mut db_views = view_map.get_all_views(&txn).into_iter().collect::<Vec<_>>();

      for db_view in db_views.iter_mut() {
        let new_view_id = if db_view.id == publish_view_id {
          self
            .duplicated_db_main_view
            .insert(pub_db_id.clone(), new_view_id.clone());
          new_view_id.clone()
        } else {
          uuid::Uuid::new_v4().to_string()
        };
        self
          .duplicated_db_view
          .insert(db_view.id.clone(), new_view_id.clone());

        db_view.id.clone_from(&new_view_id);
        db_view.database_id.clone_from(&new_db_id);
        new_db_view_ids.push(db_view.id.clone());

        // update all views's row's id
        for row_order in db_view.row_orders.iter_mut() {
          if let Some(new_id) = self
            .duplicated_refs
            .get(row_order.id.as_str())
            .cloned()
            .flatten()
          {
            row_order.id = new_id.into();
          } else {
            // skip if row not found
            tracing::warn!("row not found: {}", row_order.id);
            continue;
          }
        }
      }

      // insert updated views back to db
      view_map.clear(&mut txn);
      for view in db_views {
        view_map.insert_view(&mut txn, view);
      }
    }

    // write database collab to storage
    let db_encoded_collab = db_collab
      .encode_collab_v1(|collab| CollabType::Database.validate_require_data(collab))
      .map_err(|e| AppError::Unhandled(e.to_string()))?
      .encode_to_bytes()?;
    self
      .insert_collab_for_duplicator(&new_db_id, db_encoded_collab, CollabType::Database, pg_txn)
      .await?;

    // Add this database as linked view
    self
      .workspace_databases
      .insert(new_db_id.clone(), new_db_view_ids);

    Ok((pub_db_id, new_db_id, false))
  }

  /// Deep copy a published database to the destination workspace.
  /// Returns the Folder view for main view (`new_view_id`) and map from old to new view_id.
  /// If the database is already duplicated before, does not return the view with `new_view_id`
  async fn deep_copy_database_view<'a>(
    &mut self,
    pg_txn: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    new_view_id: String,
    published_db: PublishDatabaseData,
    metadata: &PublishViewMetaData,
    pub_view_id: &str,
  ) -> Result<View, AppError> {
    // flatten nested view info into a map
    let view_info_by_id = view_info_by_view_id(metadata);

    let (pub_db_id, _dup_db_id, db_alr_duplicated) = self
      .deep_copy_database(pg_txn, &published_db, pub_view_id, new_view_id)
      .await?;

    if db_alr_duplicated {
      let duplicated_view_id = self
        .duplicated_db_view
        .get(pub_view_id)
        .cloned()
        .ok_or_else(|| AppError::RecordNotFound(format!("view not found: {}", pub_view_id)))?;

      // db_view_id found but may not have been created due to visibility
      match self.views_to_add.get(&duplicated_view_id) {
        Some(v) => return Ok(v.clone()),
        None => {
          let main_view_id = self
            .duplicated_db_main_view
            .get(pub_db_id.as_str())
            .ok_or_else(|| {
              AppError::RecordNotFound(format!("main view not found: {}", pub_view_id))
            })?;

          let view_info = view_info_by_id.get(main_view_id).ok_or_else(|| {
            AppError::RecordNotFound(format!("metadata not found for view: {}", main_view_id))
          })?;

          let mut view =
            self.new_folder_view(duplicated_view_id, view_info, view_info.layout.clone());
          view.parent_view_id.clone_from(main_view_id);
          return Ok(view);
        },
      };
    } else {
      tracing::warn!("database not duplicated: {}", pub_view_id);
    }

    // create a new view to be returned to the caller
    // view_id is the main view of the database
    // create the main view
    let main_view_id = self
      .duplicated_db_main_view
      .get(pub_db_id.as_str())
      .ok_or_else(|| AppError::RecordNotFound(format!("main view not found: {}", pub_view_id)))?;

    let main_view_info = view_info_by_id.get(pub_view_id).ok_or_else(|| {
      AppError::RecordNotFound(format!("metadata not found for view: {}", pub_view_id))
    })?;
    let main_folder_view = self.new_folder_view(
      main_view_id.clone(),
      main_view_info,
      main_view_info.layout.clone(),
    );

    // create other visible view which are child to the main view
    for vis_view_id in published_db.visible_database_view_ids {
      if vis_view_id == pub_view_id {
        // skip main view
        continue;
      }

      let child_view_id = self
        .duplicated_db_view
        .get(&vis_view_id)
        .ok_or_else(|| AppError::RecordNotFound(format!("view not found: {}", vis_view_id)))?;

      let child_view_info = view_info_by_id.get(&vis_view_id).ok_or_else(|| {
        AppError::RecordNotFound(format!("metadata not found for view: {}", vis_view_id))
      })?;

      let mut child_folder_view = self.new_folder_view(
        child_view_id.clone(),
        view_info_by_id.get(&vis_view_id).ok_or_else(|| {
          AppError::RecordNotFound(format!("metadata not found for view: {}", vis_view_id))
        })?,
        child_view_info.layout.clone(),
      );
      child_folder_view.parent_view_id.clone_from(main_view_id);
      self
        .views_to_add
        .insert(child_folder_view.id.clone(), child_folder_view);
    }

    Ok(main_folder_view)
  }

  /// ceates a new folder view without parent_view_id set
  fn new_folder_view(
    &self,
    new_view_id: String,
    view_info: &PublishViewInfo,
    layout: ViewLayout,
  ) -> View {
    View {
      id: new_view_id,
      parent_view_id: "".to_string(), // to be filled by caller
      name: view_info.name.clone(),
      desc: "".to_string(), // unable to get from metadata
      children: RepeatedViewIdentifier { items: vec![] }, // fill in while iterating children
      created_at: self.ts_now,
      is_favorite: false,
      layout: to_folder_view_layout(layout),
      icon: view_info.icon.clone().map(to_folder_view_icon),
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
  async fn broadcast_update(&self, oid: &str, encoded_update: Vec<u8>) -> Result<(), AppError> {
    tracing::info!("broadcasting update to group: {}", oid);

    let payload = Message::Sync(SyncMessage::Update(encoded_update)).encode_v1();
    let msg = ClientCollabMessage::ClientUpdateSync {
      data: UpdateSync {
        origin: CollabOrigin::Server,
        object_id: oid.to_string(),
        msg_id: self.ts_now as u64,
        payload: payload.into(),
      },
    };

    self
      .collab_storage
      .broadcast_encode_collab(self.duplicator_uid, oid.to_string(), vec![msg])
      .await?;

    Ok(())
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
  add_to_view_info(&mut acc, &meta.child_views);
  add_to_view_info(&mut acc, &meta.ancestor_views);
  acc
}

fn add_to_view_info(acc: &mut HashMap<String, PublishViewInfo>, view_infos: &[PublishViewInfo]) {
  for view_info in view_infos {
    acc.insert(view_info.view_id.clone(), view_info.clone());
    if let Some(child_views) = &view_info.child_views {
      add_to_view_info(acc, child_views);
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

fn get_database_id_from_collab(db_collab: &Collab) -> Result<String, AppError> {
  let txn = db_collab.context.transact();
  let db_map = db_collab
    .get_with_txn(&txn, "database")
    .ok_or_else(|| AppError::RecordNotFound("no database found in database collab".to_string()))?
    .cast::<MapRef>()
    .map_err(|err| AppError::RecordNotFound(format!("database not a map: {:?}", err)))?;
  let db_id = db_map
    .get(&txn, "id")
    .ok_or_else(|| AppError::RecordNotFound("no id found in database".to_string()))?
    .to_string(&txn);
  Ok(db_id)
}

fn to_folder_view_icon(icon: workspace_dto::ViewIcon) -> collab_folder::ViewIcon {
  collab_folder::ViewIcon {
    ty: to_folder_view_icon_type(icon.ty),
    value: icon.value,
  }
}

fn to_folder_view_icon_type(icon: workspace_dto::IconType) -> collab_folder::IconType {
  match icon {
    workspace_dto::IconType::Emoji => collab_folder::IconType::Emoji,
    workspace_dto::IconType::Url => collab_folder::IconType::Url,
    workspace_dto::IconType::Icon => collab_folder::IconType::Icon,
  }
}

fn to_folder_view_layout(layout: workspace_dto::ViewLayout) -> collab_folder::ViewLayout {
  match layout {
    ViewLayout::Document => collab_folder::ViewLayout::Document,
    ViewLayout::Grid => collab_folder::ViewLayout::Grid,
    ViewLayout::Board => collab_folder::ViewLayout::Board,
    ViewLayout::Calendar => collab_folder::ViewLayout::Calendar,
    ViewLayout::Chat => collab_folder::ViewLayout::Chat,
  }
}
