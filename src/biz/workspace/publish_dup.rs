use std::{collections::HashMap, sync::Arc};

use app_error::AppError;
use appflowy_collaborate::collab::storage::CollabAccessControlStorage;
use collab::core::collab::DataSource;
use collab_document::document::Document;
use collab_entity::CollabType;
use collab_folder::{
  CollabOrigin, Folder, RepeatedViewIdentifier, View, ViewIdentifier, ViewLayout,
};
use database::{collab::CollabStorage, publish::select_published_collab_doc_state_for_view_id};
use database_entity::dto::CollabParams;
use sqlx::PgPool;

use crate::biz::collab::ops::get_latest_collab_folder_encoded;
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
  collab_type: CollabType,
) -> Result<(), AppError> {
  let copier = PublishCollabDuplicator::new(
    pg_pool.clone(),
    collab_storage.clone(),
    group_manager,
    dest_uid,
    dest_workspace_id,
    dest_view_id,
  );
  copier.deep_copy(&publish_view_id, collab_type).await?;
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

      pg_pool,
      collab_storage,
      group_manager,
      duplicator_uid: dest_uid,
      dest_workspace_id,
      dest_view_id,
    }
  }

  pub async fn deep_copy(
    mut self,
    publish_view_id: &str,
    collab_type: CollabType,
  ) -> Result<(), AppError> {
    let mut txn = self.pg_pool.begin().await?;

    // new view after deep copy
    // this is the root of the document duplicated
    let mut root_view = match self
      .deep_copy_txn(&mut txn, publish_view_id, collab_type.clone())
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
    root_view.parent_view_id = self.dest_view_id;

    let collab_folder_encoded = get_latest_collab_folder_encoded(
      self.group_manager.clone(),
      self.collab_storage.clone(),
      &self.duplicator_uid,
      &self.dest_workspace_id,
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

    // add all views required to the folder
    folder.insert_view(root_view, None);
    for view in self.views_to_add {
      folder.insert_view(view.clone(), None);
    }

    // update folder collab
    let updated_encoded_collab = folder
      .encode_collab_v1()
      .map_err(|e| AppError::Unhandled(e.to_string()))?;

    self
      .collab_storage
      .insert_or_update_collab(
        &self.dest_workspace_id,
        &self.duplicator_uid,
        CollabParams {
          object_id: self.dest_workspace_id.clone(),
          encoded_collab_v1: updated_encoded_collab.encode_to_bytes()?,
          collab_type: CollabType::Folder,
          embeddings: None,
        },
        true,
      )
      .await?;

    // broadcast to group
    // TODO(zack): broadcast update instead of stopping the group
    if let Some(group) = self.group_manager.get_group(&self.dest_workspace_id).await {
      group.stop().await;
      self
        .group_manager
        .remove_group(&self.dest_workspace_id)
        .await;
    }

    txn.commit().await?;
    Ok(())
  }

  /// Deep copy a published collab to the destination workspace.
  /// If None is returned, it means the view is not published.
  /// If Some is returned, a new view is created but without parent_view_id set.
  /// Caller should set the parent_view_id to the parent view.
  pub async fn deep_copy_txn(
    &mut self,
    txn: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    publish_view_id: &str,
    collab_type: CollabType,
  ) -> Result<Option<View>, AppError> {
    // get doc_state bin data of collab
    let doc_state: Vec<u8> =
      match select_published_collab_doc_state_for_view_id(txn, &publish_view_id.parse()?).await? {
        Some(bin_data) => bin_data,
        None => {
          tracing::warn!(
            "No published collab data found for view_id: {}",
            publish_view_id
          );
          return Ok(None);
        },
      };

    match collab_type {
      CollabType::Document => {
        let doc = Document::from_doc_state(
          CollabOrigin::Empty,
          DataSource::DocStateV1(doc_state.to_vec()),
          "",
          vec![],
        )
        .map_err(|e| AppError::Unhandled(e.to_string()))?;

        let new_doc_view = self.deep_copy_doc_txn(txn, doc).await?;
        Ok(Some(new_doc_view))
      },
      CollabType::Database => {
        // TODO
        Ok(None)
      },
      CollabType::DatabaseRow => {
        // TODO
        Ok(None)
      },
      t => {
        tracing::warn!("collab type not supported: {:?}", t);
        Ok(None)
      },
    }
  }

  pub async fn deep_copy_doc_txn<'a>(
    &mut self,
    txn: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    doc: Document,
  ) -> Result<View, AppError> {
    // create a new view
    let mut ret_view = View {
      id: uuid::Uuid::new_v4().to_string(),
      parent_view_id: "".to_string(), // to be filled by caller
      name: "duplicated".to_string(), // TODO: get from metadata
      desc: "".to_string(),           // TODO: get from metadata
      children: RepeatedViewIdentifier { items: vec![] }, // fill in while iterating children
      created_at: self.ts_now,
      is_favorite: false,
      layout: ViewLayout::Document,
      icon: None, // TODO: get from metadata
      created_by: Some(self.duplicator_uid),
      last_edited_time: self.ts_now,
      last_edited_by: Some(self.duplicator_uid),
      extra: None, // to be filled by metadata
    };

    let mut doc_data = doc
      .get_document_data()
      .map_err(|e| AppError::Unhandled(e.to_string()))?;

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
      match self.duplicated_refs.get_key_value(page_id_str) {
        Some((_old_view_id, new_view_id)) => {
          if let Some(vid) = new_view_id {
            *page_id = serde_json::json!(vid);
            ret_view
              .children
              .items
              .push(ViewIdentifier { id: vid.clone() });
          } else {
            // ref view_id is not published
            // TODO: handle this case to
            // display better in the UI?
          }
        },
        None => {
          // Call deep_copy_txn_async_wrapper and await the result
          if let Some(mut new_view) =
            Box::pin(self.deep_copy_txn(txn, page_id_str, CollabType::Document)).await?
          {
            new_view.parent_view_id = ret_view.id.clone();
            ret_view.children.items.push(ViewIdentifier {
              id: new_view.id.clone(),
            });
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
      .collab_storage
      .insert_or_update_collab(
        &self.dest_workspace_id,
        &self.duplicator_uid,
        CollabParams {
          object_id: ret_view.id.clone(),
          encoded_collab_v1: new_doc_data,
          collab_type: CollabType::Document,
          embeddings: None,
        },
        true,
      )
      .await?;

    Ok(ret_view)
  }
}
