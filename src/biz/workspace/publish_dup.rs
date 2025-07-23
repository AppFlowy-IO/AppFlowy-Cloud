use app_error::AppError;
use collab_database::database::DatabaseBody;
use collab_database::entity::FieldType;
use collab_database::rows::meta_id_from_row_id;
use collab_database::rows::DatabaseRowBody;
use collab_database::rows::RowMetaKey;
use collab_database::rows::CELL_FIELD_TYPE;
use collab_database::rows::ROW_CELLS;
use collab_database::template::entity::CELL_DATA;
use collab_database::workspace_database::WorkspaceDatabase;
use collab_document::blocks::DocumentData;
use collab_document::document::Document;
use collab_entity::CollabType;
use collab_folder::{CollabOrigin, RepeatedViewIdentifier, View};
use database::collab::GetCollabOrigin;
use database::collab::{select_workspace_database_oid, CollabStore};
use database::file::s3_client_impl::AwsS3BucketClientImpl;
use database::file::BucketClient;
use database::file::ResponseBlob;
use database::publish::select_published_data_for_view_id;
use database::publish::select_published_metadata_for_view_id;
use database_entity::dto::CollabParams;
use shared_entity::dto::publish_dto::PublishDatabaseDataWithNonUuidRelations;
use shared_entity::dto::publish_dto::{PublishDatabaseData, PublishViewInfo, PublishViewMetaData};
use shared_entity::dto::workspace_dto::ViewLayout;
use sqlx::PgPool;
use std::collections::HashSet;
use std::time::{Duration, Instant};
use std::{collections::HashMap, sync::Arc};

use crate::biz::collab::folder_view::to_folder_view_icon;
use crate::biz::collab::folder_view::to_folder_view_layout;
use crate::biz::collab::utils::{collab_from_doc_state, get_latest_collab};
use tracing::error;
use uuid::Uuid;
use workspace_template::gen_view_id;
use yrs::Any;
use yrs::Array;
use yrs::ArrayRef;
use yrs::Out;
use yrs::{Map, MapRef};

use crate::biz::collab::utils::collab_to_bin;

use crate::state::AppState;
use appflowy_collaborate::ws2::{CollabUpdatePublisher, WorkspaceCollabInstanceCache};
use appflowy_collaborate::CollabMetrics;
use collab::core::collab::default_client_id;
use collab_database::database_trait::NoPersistenceDatabaseCollabService;

#[allow(clippy::too_many_arguments)]
pub async fn duplicate_published_collab_to_workspace(
  state: &AppState,
  dest_uid: i64,
  publish_view_id: Uuid,
  dest_workspace_id: Uuid,
  dest_view_id: Uuid,
) -> Result<Uuid, AppError> {
  let copier = PublishCollabDuplicator::new(
    state.pg_pool.clone(),
    state.bucket_client.clone(),
    state.collab_storage.clone(),
    Box::new(state.ws_server.clone()),
    dest_uid,
    dest_workspace_id,
    dest_view_id,
    state.metrics.collab_metrics.clone(),
  );

  let time_now = chrono::Utc::now().timestamp_millis();
  let root_view_id_for_duplicate = copier.duplicate(publish_view_id, &state.ws_server).await?;
  let elapsed = chrono::Utc::now().timestamp_millis() - time_now;
  tracing::info!(
    "duplicate_published_collab_to_workspace: elapsed time: {}ms",
    elapsed
  );
  Ok(root_view_id_for_duplicate)
}

pub struct PublishCollabDuplicator {
  /// for fetching and writing folder data
  /// of dest workspace
  collab_storage: Arc<dyn CollabStore>,
  /// A map to store the old view_id that was duplicated and new view_id assigned.
  /// If value is none, it means the view_id is not published.
  duplicated_refs: HashMap<Uuid, Option<Uuid>>,
  /// published_database_id -> view_id
  duplicated_db_main_view: HashMap<Uuid, Uuid>,
  /// published_database_view_id -> new_view_id
  duplicated_db_view: HashMap<Uuid, Uuid>,
  /// published_database_row_id -> new_row_id
  duplicated_db_row: HashMap<Uuid, Uuid>,
  /// new views to be added to the folder
  /// view_id -> view
  views_to_add: HashMap<Uuid, View>,
  /// A list of database linked views to be added to workspace database
  workspace_databases: HashMap<Uuid, Vec<Uuid>>,
  /// A list of collab objects to added to the workspace (oid -> collab)
  collabs_to_insert: HashMap<Uuid, (CollabType, Vec<u8>)>,
  /// time of duplication
  ts_now: i64,
  /// for fetching published data
  /// and writing them to dest workspace
  pg_pool: PgPool,
  /// for fetching published data from s3
  bucket_client: AwsS3BucketClientImpl,
  /// user initiating the duplication
  duplicator_uid: i64,
  /// workspace to duplicate into
  dest_workspace_id: Uuid,
  /// view of workspace to duplicate into
  dest_view_id: Uuid,
  collab_update_publisher: Box<dyn CollabUpdatePublisher>,
  collab_metrics: Arc<CollabMetrics>,
}

fn deserialize_publish_database_data(
  published_blob: &[u8],
) -> Result<PublishDatabaseData, AppError> {
  match serde_json::from_slice::<PublishDatabaseData>(published_blob) {
    Ok(payload) => Ok(payload),
    Err(_) => {
      match serde_json::from_slice::<PublishDatabaseDataWithNonUuidRelations>(published_blob) {
        Ok(payload) => Ok(payload.into()),
        Err(err) => Err(AppError::from(err)),
      }
    },
  }
}

impl PublishCollabDuplicator {
  #[allow(clippy::too_many_arguments)]
  pub fn new(
    pg_pool: PgPool,
    bucket_client: AwsS3BucketClientImpl,
    collab_storage: Arc<dyn CollabStore>,

    collab_update_publisher: Box<dyn CollabUpdatePublisher>,
    dest_uid: i64,
    dest_workspace_id: Uuid,
    dest_view_id: Uuid,
    collab_metrics: Arc<CollabMetrics>,
  ) -> Self {
    let ts_now = chrono::Utc::now().timestamp();
    Self {
      ts_now,
      duplicated_refs: HashMap::new(),
      views_to_add: HashMap::new(),
      workspace_databases: HashMap::new(),
      collabs_to_insert: HashMap::new(),
      duplicated_db_main_view: HashMap::new(),
      duplicated_db_view: HashMap::new(),
      duplicated_db_row: HashMap::new(),
      pg_pool,
      bucket_client,
      collab_storage,
      duplicator_uid: dest_uid,
      dest_workspace_id,
      dest_view_id,
      collab_update_publisher,
      collab_metrics,
    }
  }

  async fn duplicate(
    mut self,
    publish_view_id: Uuid,
    collab_instance_cache: &impl WorkspaceCollabInstanceCache,
  ) -> Result<Uuid, AppError> {
    // new view after deep copy
    // this is the root of the document/database duplicated
    let root_view_id = gen_view_id();
    let mut root_view = match self.deep_copy(root_view_id, publish_view_id).await? {
      Some(v) => v,
      None => {
        return Err(AppError::RecordNotFound(
          "view not found, it might be unpublished".to_string(),
        ))
      },
    };
    root_view.parent_view_id = self.dest_view_id.to_string();

    // destructuring self to own inner values, avoids cloning
    let PublishCollabDuplicator {
      collab_storage,
      duplicated_refs: _,
      duplicated_db_main_view: _,
      duplicated_db_view: _,
      duplicated_db_row: _,
      mut views_to_add,
      workspace_databases,
      collabs_to_insert,
      ts_now: _,
      pg_pool,
      bucket_client: _,
      duplicator_uid,
      dest_workspace_id,
      dest_view_id,
      collab_update_publisher: collab_update_writer,
      collab_metrics,
    } = self;

    // insert all collab object accumulated
    // for self.collabs_to_insert
    let mut txn = pg_pool.begin().await?;
    let start = Instant::now();
    for (oid, (collab_type, encoded_collab)) in collabs_to_insert.into_iter() {
      let params = CollabParams {
        object_id: oid,
        encoded_collab_v1: encoded_collab.into(),
        collab_type,
        updated_at: None,
      };
      let action = format!("duplicate collab: {}", params);
      collab_storage
        .upsert_new_collab_with_transaction(
          dest_workspace_id,
          &duplicator_uid,
          params,
          &mut txn,
          &action,
        )
        .await?;
    }
    match tokio::time::timeout(Duration::from_secs(60), txn.commit()).await {
      Ok(result) => {
        collab_metrics.observe_pg_tx(start.elapsed());
        result.map_err(AppError::from)
      },
      Err(_) => {
        error!("Timeout waiting for duplicating collabs");
        Err(AppError::RequestTimeout(
          "timeout while duplicating".to_string(),
        ))
      },
    }?;

    // update database if any
    if !workspace_databases.is_empty() {
      let ws_db_oid = select_workspace_database_oid(&pg_pool, &dest_workspace_id).await?;
      let ws_db_collab = {
        get_latest_collab(
          &collab_storage,
          GetCollabOrigin::User {
            uid: duplicator_uid,
          },
          dest_workspace_id,
          ws_db_oid,
          CollabType::WorkspaceDatabase,
          default_client_id(),
        )
        .await?
      };

      let mut ws_db = WorkspaceDatabase::open(ws_db_collab).map_err(|err| {
        AppError::Unhandled(format!("failed to open workspace database: {}", err))
      })?;

      let view_ids_by_database_id = workspace_databases
        .into_iter()
        .map(|(database_id, view_ids)| {
          (
            database_id.to_string(),
            view_ids
              .into_iter()
              .map(|view_id| view_id.to_string())
              .collect(),
          )
        })
        .collect::<HashMap<_, _>>();

      let workspace_database_update = ws_db
        .batch_add_database(view_ids_by_database_id)
        .encode_update_v1();
      collab_update_writer
        .publish_update(
          dest_workspace_id,
          ws_db_oid,
          CollabType::WorkspaceDatabase,
          &CollabOrigin::Server,
          workspace_database_update,
        )
        .await?;
    }

    let mut folder = collab_instance_cache.get_folder(dest_workspace_id).await?;
    let folder_updates = tokio::task::spawn_blocking(move || {
      let mut folder_txn = folder.collab.transact_mut();
      let mut duplicated_view_ids = HashSet::new();
      duplicated_view_ids.insert(dest_view_id);
      duplicated_view_ids.insert(root_view.id.parse().unwrap());
      folder
        .body
        .views
        .insert(&mut folder_txn, root_view, None, duplicator_uid);
      // when child views are added, it must have a parent view that is previously added
      // TODO: if there are too many child views, consider using topological sort
      loop {
        if views_to_add.is_empty() {
          break;
        }

        let mut inserted = vec![];
        for (view_id, view) in views_to_add.iter() {
          let parent_view_id = Uuid::parse_str(&view.parent_view_id).unwrap();
          // allow to insert if parent view is already inserted
          // or if view is standalone (view_id == parent_view_id)
          if duplicated_view_ids.contains(&parent_view_id) || *view_id == parent_view_id {
            folder
              .body
              .views
              .insert(&mut folder_txn, view.clone(), None, duplicator_uid);
            duplicated_view_ids.insert(*view_id);
            inserted.push(*view_id);
          }
        }
        if inserted.is_empty() {
          tracing::error!(
            "views not inserted because parent_id does not exists: {:?}",
            views_to_add.keys()
          );
          break;
        }
        for view_id in inserted {
          views_to_add.remove(&view_id);
        }
      }

      folder_txn.encode_update_v1()
    })
    .await?;
    collab_update_writer
      .publish_update(
        dest_workspace_id,
        dest_workspace_id,
        CollabType::Folder,
        &CollabOrigin::Server,
        folder_updates,
      )
      .await?;

    Ok(root_view_id)
  }

  /// Deep copy a published collab to the destination workspace.
  /// If None is returned, it means the view is not published.
  /// If Some is returned, a new view is created but without parent_view_id set.
  /// Caller should set the parent_view_id to the parent view.
  async fn deep_copy(
    &mut self,
    new_view_id: Uuid,
    publish_view_id: Uuid,
  ) -> Result<Option<View>, AppError> {
    tracing::info!(
      "deep_copy: new_view_id: {}, publish_view_id: {}",
      new_view_id,
      publish_view_id,
    );

    // attempt to get metadata and doc_state for published view
    let (metadata, published_blob) = match self
      .get_published_data_for_view_id(&publish_view_id)
      .await?
    {
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
      .insert(publish_view_id, new_view_id.into());

    match metadata.view.layout {
      ViewLayout::Document => {
        let doc_collab =
          collab_from_doc_state(published_blob, &Uuid::default(), default_client_id())?;
        let doc = Document::open(doc_collab).map_err(|e| AppError::Unhandled(e.to_string()))?;
        let new_doc_view = self
          .deep_copy_doc(publish_view_id, new_view_id, doc, metadata)
          .await?;
        Ok(Some(new_doc_view))
      },
      ViewLayout::Grid | ViewLayout::Board | ViewLayout::Calendar => {
        let pub_view_id: Uuid = metadata.view.view_id.parse()?;
        let db_payload = deserialize_publish_database_data(&published_blob)?;
        let new_db_view = self
          .deep_copy_database_view(new_view_id, db_payload, &metadata, &pub_view_id)
          .await?;
        Ok(Some(new_db_view))
      },
      t => {
        tracing::warn!("collab type not supported: {:?}", t);
        Ok(None)
      },
    }
  }

  async fn deep_copy_doc(
    &mut self,
    pub_view_id: Uuid,
    dup_view_id: Uuid,
    doc: Document,
    metadata: PublishViewMetaData,
  ) -> Result<View, AppError> {
    let mut ret_view = self.new_folder_view(dup_view_id, &metadata.view, ViewLayout::Document);

    let mut doc_data = doc
      .get_document_data()
      .map_err(|e| AppError::Unhandled(e.to_string()))?;

    if let Err(err) = self.deep_copy_doc_pages(&mut doc_data, &mut ret_view).await {
      tracing::error!("failed to deep copy doc pages: {}", err);
    }

    if let Err(err) = self
      .deep_copy_doc_databases(&pub_view_id, &mut doc_data, &mut ret_view)
      .await
    {
      tracing::error!("failed to deep copy doc databases: {}", err);
    };

    {
      // write modified doc_data back to storage
      let empty_collab = collab_from_doc_state(vec![], &dup_view_id, default_client_id())?;
      let new_doc = tokio::task::spawn_blocking(move || {
        Document::create_with_data(empty_collab, doc_data)
          .map_err(|e| AppError::Unhandled(e.to_string()))
      })
      .await??;
      let new_doc_bin = collab_to_bin(new_doc.split().0, CollabType::Document).await?;
      self
        .collabs_to_insert
        .insert(ret_view.id.parse()?, (CollabType::Document, new_doc_bin));
    }
    Ok(ret_view)
  }

  async fn deep_copy_doc_pages(
    &mut self,
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
            mention
              .get("type")
              .is_some_and(|type_| type_.as_str() == Some("page"))
          })
          .flat_map(|mention| mention.get_mut("page_id"));

        for page_id in page_ids {
          let page_id_str = match page_id.as_str() {
            Some(page_id_str) => Uuid::parse_str(page_id_str)?,
            None => continue,
          };
          if let Some(new_page_id) = self
            .deep_copy_view(page_id_str, ret_view.id.parse()?)
            .await?
          {
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

  /// Attempts to deep copy a view using `pub_view_id`.
  /// Returns None if view is not published else
  /// returns the view id of the duplicated view.
  /// If view is already duplicated, returns duplicated view's view_id (parent_view_id is not set
  /// from param `parent_view_id`)
  async fn deep_copy_view(
    &mut self,
    pub_view_id: Uuid,
    parent_view_id: Uuid,
  ) -> Result<Option<Uuid>, AppError> {
    match self.duplicated_refs.get(&pub_view_id) {
      Some(new_view_id) => {
        if let Some(vid) = new_view_id {
          Ok(Some(*vid))
        } else {
          Ok(None)
        }
      },
      None => {
        // Call deep_copy and await the result
        if let Some(mut new_view) = Box::pin(self.deep_copy(gen_view_id(), pub_view_id)).await? {
          if new_view.parent_view_id.is_empty() {
            new_view.parent_view_id = parent_view_id.to_string();
          }
          let new_view_id = Uuid::parse_str(&new_view.id)?;
          self.duplicated_refs.insert(pub_view_id, Some(new_view_id));
          self.views_to_add.insert(new_view_id, new_view);
          Ok(Some(new_view_id))
        } else {
          tracing::warn!("view not found in deep_copy: {}", pub_view_id);
          self.duplicated_refs.insert(pub_view_id, None);
          Ok(None)
        }
      },
    }
  }

  async fn deep_copy_doc_databases(
    &mut self,
    pub_view_id: &Uuid,
    doc_data: &mut DocumentData,
    ret_view: &mut View,
  ) -> Result<(), AppError> {
    let db_blocks = doc_data
      .blocks
      .iter_mut()
      .filter(|(_, b)| b.ty == "grid" || b.ty == "board" || b.ty == "calendar");

    for (block_id, block) in db_blocks {
      tracing::info!("deep_copy_doc_databases: block_id: {}", block_id);
      let block_view_id: Uuid = block
        .data
        .get("view_id")
        .ok_or_else(|| AppError::RecordNotFound("view_id not found in block data".to_string()))?
        .as_str()
        .ok_or_else(|| AppError::RecordNotFound("view_id not a string".to_string()))?
        .parse()?;

      let block_parent_id: Uuid = block
        .data
        .get("parent_id")
        .ok_or_else(|| AppError::RecordNotFound("view_id not found in block data".to_string()))?
        .as_str()
        .ok_or_else(|| AppError::RecordNotFound("view_id not a string".to_string()))?
        .parse()?;

      if pub_view_id == &block_parent_id {
        // inline database in doc
        if let Some(new_view_id) = self
          .deep_copy_inline_database_in_doc(block_view_id, &ret_view.id)
          .await?
        {
          block.data.insert(
            "view_id".to_string(),
            serde_json::Value::String(new_view_id),
          );
          block.data.insert(
            "parent_id".to_string(),
            serde_json::Value::String(ret_view.id.clone()),
          );
        } else {
          tracing::warn!("deep_copy_doc_databases: view not found: {}", block_view_id);
        }
      } else {
        // reference to database
        if let Some((new_view_id, new_parent_id)) = self
          .deep_copy_ref_database_in_doc(block_view_id, block_parent_id, &ret_view.id)
          .await?
        {
          block.data.insert(
            "view_id".to_string(),
            serde_json::Value::String(new_view_id.to_string()),
          );
          block.data.insert(
            "parent_id".to_string(),
            serde_json::Value::String(new_parent_id.to_string()),
          );
        } else {
          tracing::warn!("deep_copy_doc_databases: view not found: {}", block_view_id);
        }
      }
    }

    Ok(())
  }

  /// deep copy inline database for doc
  /// returns new view_id
  /// parent_view_id is assumed to be doc itself
  async fn deep_copy_inline_database_in_doc(
    &mut self,
    view_id: Uuid,
    doc_view_id: &str,
  ) -> Result<Option<String>, AppError> {
    let (metadata, published_blob) = match self.get_published_data_for_view_id(&view_id).await? {
      Some(published_data) => published_data,
      None => {
        tracing::warn!("No published collab data found for view_id: {}", view_id);
        return Ok(None);
      },
    };

    let published_db = deserialize_publish_database_data(&published_blob)?;
    let mut parent_view = self
      .deep_copy_database_view(gen_view_id(), published_db, &metadata, &view_id)
      .await?;
    let parent_view_id = parent_view.id.clone();
    if parent_view.parent_view_id.is_empty() {
      parent_view.parent_view_id = doc_view_id.to_string();
      let parent_view_id = Uuid::parse_str(&parent_view.id)?;
      self.views_to_add.insert(parent_view_id, parent_view);
    }
    Ok(Some(parent_view_id))
  }

  /// deep copy referenced database for doc
  /// returns new (view_id, parent_id)
  async fn deep_copy_ref_database_in_doc(
    &mut self,
    view_id: Uuid,
    parent_id: Uuid,
    doc_view_id: &String,
  ) -> Result<Option<(Uuid, Uuid)>, AppError> {
    let (metadata, published_blob) = match self.get_published_data_for_view_id(&view_id).await? {
      Some(published_data) => published_data,
      None => {
        tracing::warn!("No published collab data found for view_id: {}", view_id);
        return Ok(None);
      },
    };

    let published_db = serde_json::from_slice::<PublishDatabaseData>(&published_blob)?;
    let mut parent_view = self
      .deep_copy_database_view(gen_view_id(), published_db, &metadata, &parent_id)
      .await?;
    let parent_view_id: Uuid = parent_view.id.parse()?;
    if parent_view.parent_view_id.is_empty() {
      parent_view.parent_view_id = doc_view_id.to_string();
      self.views_to_add.insert(parent_view_id, parent_view);
    }
    let duplicated_view_id = match self.duplicated_db_view.get(&view_id) {
      Some(v) => *v,
      None => {
        let view_info_by_id = view_info_by_view_id(&metadata);
        let view_info = view_info_by_id.get(&view_id).ok_or_else(|| {
          AppError::RecordNotFound(format!("metadata not found for view: {}", view_id))
        })?;
        let mut new_folder_db_view =
          self.new_folder_view(view_id, view_info, view_info.layout.clone());
        new_folder_db_view.parent_view_id = parent_view_id.to_string();
        let new_folder_db_view_id: Uuid = new_folder_db_view.id.parse()?;
        self
          .views_to_add
          .insert(new_folder_db_view_id, new_folder_db_view);
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
  async fn deep_copy_database(
    &mut self,
    published_db: &PublishDatabaseData,
    pub_view_id: &Uuid,
    new_view_id: Uuid,
  ) -> Result<(Uuid, Uuid, bool), AppError> {
    // collab of database
    let client_id = default_client_id();
    let mut db_collab = collab_from_doc_state(
      published_db.database_collab.clone(),
      &Uuid::default(),
      client_id,
    )?;
    let db_body = DatabaseBody::from_collab(
      &db_collab,
      Arc::new(NoPersistenceDatabaseCollabService::new(client_id)),
      None,
    )
    .ok_or_else(|| AppError::RecordNotFound("no database body found".to_string()))?;
    let pub_db_id: Uuid = db_body
      .get_database_id(&db_collab.context.transact())
      .parse()?;

    // check if the database is already duplicated
    if let Some(db_id) = self.duplicated_refs.get(&pub_db_id).cloned().flatten() {
      return Ok((pub_db_id, db_id, true));
    }
    let new_db_id = gen_view_id();
    self.duplicated_refs.insert(pub_db_id, Some(new_db_id));

    {
      // assign new id to all views of database.
      // this will mark the database as duplicated
      let txn = db_collab.context.transact();
      let mut db_views = db_body.views.get_all_views(&txn);
      let mut new_db_view_ids: Vec<_> = Vec::with_capacity(db_views.len());
      for db_view in db_views.iter_mut() {
        let db_view_id: Uuid = db_view.id.parse()?;
        let new_db_view_id = if &db_view_id == pub_view_id {
          self.duplicated_db_main_view.insert(pub_db_id, new_view_id);
          new_view_id
        } else {
          gen_view_id()
        };
        self.duplicated_db_view.insert(db_view_id, new_db_view_id);

        new_db_view_ids.push(new_db_view_id);
      }
      // if there is no main view id, use the inline view id
      if let std::collections::hash_map::Entry::Vacant(e) =
        self.duplicated_db_main_view.entry(pub_db_id)
      {
        e.insert(db_body.get_inline_view_id(&txn).parse()?);
      };

      // Add this database as linked view
      self.workspace_databases.insert(new_db_id, new_db_view_ids);
    }

    // assign new id to all rows of database.
    // this will mark the rows as duplicated
    for pub_row_id in published_db.database_row_collabs.keys() {
      // assign a new id for the row
      let dup_row_id = Uuid::new_v4();
      self.duplicated_db_row.insert(*pub_row_id, dup_row_id);
    }

    {
      // handle row relations
      let mut txn = db_collab.context.transact_mut();
      let all_fields = db_body.fields.get_all_fields(&txn);
      for mut field in all_fields {
        for (key, type_option_value) in field.type_options.iter_mut() {
          if *key == FieldType::Relation.type_id() {
            if let Some(pub_db_id) = type_option_value.get_mut("database_id") {
              if let Any::String(pub_db_id_str) = pub_db_id {
                let pub_db_uuid = Uuid::parse_str(pub_db_id_str)?;
                if let Some(&pub_rel_db_view) = published_db.database_relations.get(&pub_db_uuid) {
                  if let Some(_dup_view_id) = self
                    .deep_copy_view(pub_rel_db_view, self.dest_view_id)
                    .await?
                  {
                    if let Some(dup_db_id) =
                      self.duplicated_refs.get(&pub_db_uuid).cloned().flatten()
                    {
                      *pub_db_id = Any::from(dup_db_id.to_string());
                      db_body.fields.update_field(&mut txn, &field.id, |f| {
                        f.set_type_option(
                          FieldType::Relation.into(),
                          Some(type_option_value.clone()),
                        );
                      });
                    };
                  };
                };
              }
            }
          }
        }
      }
    }

    // duplicate db collab rows
    for (pub_row_id, row_bin_data) in &published_db.database_row_collabs {
      let dup_row_id = *self
        .duplicated_db_row
        .get(pub_row_id)
        .ok_or_else(|| AppError::RecordNotFound(format!("row not found: {}", pub_row_id)))?;

      let mut db_row_collab =
        collab_from_doc_state(row_bin_data.clone(), &dup_row_id, default_client_id())?;
      let mut db_row_body = DatabaseRowBody::open((*pub_row_id).into(), &mut db_row_collab)
        .map_err(|e| AppError::Unhandled(e.to_string()))?;

      {
        let mut txn = db_row_collab.context.transact_mut();
        // update database_id
        db_row_body.update(&mut txn, |u| {
          u.set_database_id(new_db_id.to_string());
        });

        // get row document id before the id update
        let pub_row_doc_id = db_row_body
          .document_id(&txn)
          .map_err(|e| AppError::Unhandled(e.to_string()))?;

        // updates row id along with meta keys
        db_row_body
          .update_id(&mut txn, dup_row_id.into())
          .map_err(|e| AppError::Unhandled(format!("failed to update row id: {:?}", e)))?;

        // duplicate row document if exists
        if let Some(pub_row_doc_id) = pub_row_doc_id {
          let pub_row_doc_id: Uuid = pub_row_doc_id.parse()?;
          if let Some(row_doc_doc_state) = published_db
            .database_row_document_collabs
            .get(&pub_row_doc_id)
          {
            match collab_from_doc_state(
              row_doc_doc_state.to_vec(),
              &pub_row_doc_id,
              default_client_id(),
            ) {
              Ok(pub_doc_collab) => {
                let pub_doc =
                  Document::open(pub_doc_collab).map_err(|e| AppError::Unhandled(e.to_string()))?;
                let dup_row_doc_id =
                  meta_id_from_row_id(&dup_row_id, RowMetaKey::DocumentId).parse()?;
                let mut new_doc_view = Box::pin(self.deep_copy_doc(
                  pub_row_doc_id,
                  dup_row_doc_id,
                  pub_doc,
                  PublishViewMetaData::default(),
                ))
                .await?;
                new_doc_view.parent_view_id = dup_row_doc_id.to_string(); // orphan folder view
                self.views_to_add.insert(dup_row_doc_id, new_doc_view);
              },
              Err(err) => tracing::error!("failed to open row document: {}", err),
            };
          } else {
            tracing::error!("no document found for row: {}", pub_row_doc_id);
          };
        }

        {
          // "cells": Object {
          //     "MBaTsr": Object {
          //         "data": Array [
          //             String("eefb5700-8cf7-411e-9596-f60b9a51916e"),
          //             String("23d5e054-42c8-4754-ad69-527e4ffc1e46"),
          //             // above are published row ids of related database
          //             // we need to replace them with respective duplicated row ids
          //         ],
          //         "field_type": Number(10),
          //         // use this condition to filter out relation cells
          //     },
          // },
          let cells: MapRef = db_row_body
            .get_data()
            .get(&txn, ROW_CELLS)
            .ok_or_else(|| {
              AppError::RecordNotFound("no cells found in database row collab".to_string())
            })?
            .cast()
            .map_err(|e| AppError::Unhandled(format!("not a map: {:?}", e)))?;

          // collect all cell with field type as relation
          let mut rel_row_idss = vec![];
          for (_, out) in cells.iter(&txn) {
            if let Ok(m) = out.cast::<MapRef>() {
              if let Some(Out::Any(Any::BigInt(n))) = m.get(&txn, CELL_FIELD_TYPE) {
                if n == FieldType::Relation as i64 {
                  match m.get(&txn, CELL_DATA) {
                    Some(relation_data) => {
                      if let Ok(arr) = relation_data.cast::<ArrayRef>() {
                        rel_row_idss.push(arr)
                      };
                    },
                    None => {
                      tracing::warn!("no data found in relation cell, pub_row_id: {}", pub_row_id)
                    },
                  }
                }
              }
            }
          }
          // replace all relation cells with duplicated row ids
          for rel_row_ids in rel_row_idss {
            let num_refs = rel_row_ids.len(&txn);
            let mut pub_row_ids = Vec::with_capacity(num_refs as usize);
            for rel_row_id in rel_row_ids.iter(&txn) {
              if let Out::Any(Any::String(s)) = rel_row_id {
                pub_row_ids.push(s);
              }
            }
            rel_row_ids.remove_range(&mut txn, 0, num_refs);
            for pub_row_id in pub_row_ids {
              let pub_row_id = Uuid::parse_str(&pub_row_id)?;
              let dup_row_id = self.duplicated_db_row.get(&pub_row_id).ok_or_else(|| {
                AppError::RecordNotFound(format!("row not found: {}", pub_row_id))
              })?;
              let _ = rel_row_ids.push_back(&mut txn, dup_row_id.to_string());
            }
          }
        }
      }

      // write new row collab to storage
      let db_row_ec_bytes = collab_to_bin(db_row_collab, CollabType::DatabaseRow).await?;
      self
        .collabs_to_insert
        .insert(dup_row_id, (CollabType::DatabaseRow, db_row_ec_bytes));
    }

    // accumulate list of database views (Board, Cal, ...) to be linked to the database
    {
      let mut txn = db_collab.context.transact_mut();
      db_body.root.insert(&mut txn, "id", new_db_id.to_string());

      let mut db_views = db_body.views.get_all_views(&txn);
      for db_view in db_views.iter_mut() {
        let db_view_id = Uuid::parse_str(&db_view.id)?;
        let new_db_view_id = self.duplicated_db_view.get(&db_view_id).ok_or_else(|| {
          AppError::Unhandled(format!(
            "view not found in duplicated_db_view: {}",
            db_view.id
          ))
        })?;

        db_view.id = new_db_view_id.to_string();
        db_view.database_id = new_db_id.to_string();

        // update all views's row's id
        for row_order in db_view.row_orders.iter_mut() {
          let row_order_id = Uuid::parse_str(&row_order.id)?;
          if let Some(new_id) = self.duplicated_db_row.get(&row_order_id) {
            row_order.id = (*new_id).into();
          } else {
            // skip if row not found
            tracing::warn!("row not found: {}", row_order.id);
            continue;
          }
        }
      }

      // update database metas iid
      db_body
        .metas
        .insert(&mut txn, "iid", new_view_id.to_string());

      // insert updated views back to db
      db_body.views.clear(&mut txn);
      for view in db_views {
        db_body.views.insert_view(&mut txn, view);
      }
    }

    // write database collab to storage
    let db_encoded_collab = collab_to_bin(db_collab, CollabType::Database).await?;
    self
      .collabs_to_insert
      .insert(new_db_id, (CollabType::Database, db_encoded_collab));

    Ok((pub_db_id, new_db_id, false))
  }

  /// Deep copy a published database to the destination workspace.
  /// Returns the Folder view for main view (`new_view_id`) and map from old to new view_id.
  /// If the database is already duplicated before, does not return the view with `new_view_id`
  async fn deep_copy_database_view(
    &mut self,
    new_view_id: Uuid,
    published_db: PublishDatabaseData,
    metadata: &PublishViewMetaData,
    pub_view_id: &Uuid,
  ) -> Result<View, AppError> {
    // flatten nested view info into a map
    let view_info_by_id = view_info_by_view_id(metadata);

    let (pub_db_id, _dup_db_id, db_alr_duplicated) = self
      .deep_copy_database(&published_db, pub_view_id, new_view_id)
      .await?;

    if db_alr_duplicated {
      let dup_view_id = self
        .duplicated_db_view
        .get(pub_view_id)
        .cloned()
        .ok_or_else(|| AppError::RecordNotFound(format!("view not found: {}", pub_view_id)))?;

      // db_view_id found but may not have been created due to visibility
      match self.views_to_add.get(&dup_view_id) {
        Some(v) => return Ok(v.clone()),
        None => {
          let main_view_id = self
            .duplicated_db_main_view
            .get(&pub_db_id)
            .ok_or_else(|| {
              AppError::RecordNotFound(format!("main view not found: {}", pub_view_id))
            })?;

          let view_info = view_info_by_id.get(pub_view_id).ok_or_else(|| {
            AppError::RecordNotFound(format!("metadata not found for view: {}", main_view_id))
          })?;

          let mut view = self.new_folder_view(dup_view_id, view_info, view_info.layout.clone());
          let main_view_id = main_view_id.to_string();
          if main_view_id != view.id {
            view.parent_view_id = main_view_id;
          }
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
      .get(&pub_db_id)
      .ok_or_else(|| AppError::RecordNotFound(format!("main view not found: {}", pub_view_id)))?;

    let main_view_info = view_info_by_id.get(pub_view_id).ok_or_else(|| {
      AppError::RecordNotFound(format!("metadata not found for view: {}", pub_view_id))
    })?;
    let main_folder_view =
      self.new_folder_view(*main_view_id, main_view_info, main_view_info.layout.clone());

    // create other visible view which are child to the main view
    for vis_view_id in published_db.visible_database_view_ids {
      if &vis_view_id == pub_view_id {
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
        *child_view_id,
        view_info_by_id.get(&vis_view_id).ok_or_else(|| {
          AppError::RecordNotFound(format!("metadata not found for view: {}", vis_view_id))
        })?,
        child_view_info.layout.clone(),
      );
      child_folder_view.parent_view_id = main_view_id.to_string();
      self
        .views_to_add
        .insert(child_folder_view.id.parse()?, child_folder_view);
    }

    Ok(main_folder_view)
  }

  /// creates a new folder view without parent_view_id set
  fn new_folder_view(
    &self,
    new_view_id: Uuid,
    view_info: &PublishViewInfo,
    layout: ViewLayout,
  ) -> View {
    View {
      id: new_view_id.to_string(),
      parent_view_id: "".to_string(), // to be filled by caller
      name: view_info.name.clone(),
      children: RepeatedViewIdentifier { items: vec![] }, // fill in while iterating children
      created_at: self.ts_now,
      is_favorite: false,
      layout: to_folder_view_layout(layout),
      icon: view_info.icon.clone().map(to_folder_view_icon),
      created_by: Some(self.duplicator_uid),
      last_edited_time: self.ts_now,
      last_edited_by: Some(self.duplicator_uid),
      is_locked: None,
      extra: view_info.extra.clone(),
    }
  }

  async fn get_published_data_for_view_id(
    &self,
    view_id: &uuid::Uuid,
  ) -> Result<Option<(PublishViewMetaData, Vec<u8>)>, AppError> {
    let result = select_published_metadata_for_view_id(&self.pg_pool, view_id).await?;
    match result {
      Some((workspace_id, js_val)) => {
        let metadata = serde_json::from_value(js_val)?;
        let object_key = format!("published-collab/{}/{}", workspace_id, view_id);
        match self.bucket_client.get_blob(&object_key).await {
          Ok(resp) => Ok(Some((metadata, resp.to_blob()))),
          Err(_) => match select_published_data_for_view_id(&self.pg_pool, view_id).await? {
            Some((js_val, blob)) => {
              let metadata = serde_json::from_value(js_val)?;
              Ok(Some((metadata, blob)))
            },
            None => Ok(None),
          },
        }
      },
      None => Ok(None),
    }
  }
}

fn view_info_by_view_id(meta: &PublishViewMetaData) -> HashMap<Uuid, PublishViewInfo> {
  let mut acc = HashMap::new();
  acc.insert(meta.view.view_id.parse().unwrap(), meta.view.clone());
  add_to_view_info(&mut acc, &meta.child_views);
  add_to_view_info(&mut acc, &meta.ancestor_views);
  acc
}

fn add_to_view_info(acc: &mut HashMap<Uuid, PublishViewInfo>, view_infos: &[PublishViewInfo]) {
  for view_info in view_infos {
    acc.insert(view_info.view_id.parse().unwrap(), view_info.clone());
    if let Some(child_views) = &view_info.child_views {
      add_to_view_info(acc, child_views);
    }
  }
}
