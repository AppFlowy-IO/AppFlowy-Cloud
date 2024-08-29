use std::sync::{Arc, Weak};
use std::time::Duration;

use anyhow::anyhow;
use collab::lock::RwLock;
use collab::preclude::Collab;
use collab_entity::{validate_data_for_folder, CollabType};
use tokio::sync::mpsc;
use tokio::time::interval;
use tracing::{trace, warn};

use app_error::AppError;
use database::collab::CollabStorage;
use database_entity::dto::CollabParams;

use crate::group::group_init::EditState;
use crate::indexer::Indexer;

pub(crate) struct GroupPersistence<S> {
  workspace_id: String,
  object_id: String,
  storage: Arc<S>,
  uid: i64,
  edit_state: Arc<EditState>,
  collab: Weak<RwLock<Collab>>,
  collab_type: CollabType,
  persistence_interval: Duration,
  indexer: Option<Arc<dyn Indexer>>,
}

impl<S> GroupPersistence<S>
where
  S: CollabStorage,
{
  #[allow(clippy::too_many_arguments)]
  pub fn new(
    workspace_id: String,
    object_id: String,
    uid: i64,
    storage: Arc<S>,
    edit_state: Arc<EditState>,
    collab: Weak<RwLock<Collab>>,
    collab_type: CollabType,
    persistence_interval: Duration,
    ai_client: Option<Arc<dyn Indexer>>,
  ) -> Self {
    Self {
      workspace_id,
      object_id,
      uid,
      storage,
      edit_state,
      collab,
      collab_type,
      persistence_interval,
      indexer: ai_client,
    }
  }

  pub async fn run(self, mut destroy_group_rx: mpsc::Receiver<Arc<RwLock<Collab>>>) {
    let mut interval = interval(self.persistence_interval);
    loop {
      // delay 30 seconds before the first save. We don't want to save immediately after the collab is created
      tokio::time::sleep(Duration::from_secs(30)).await;

      tokio::select! {
        _ = interval.tick() => {
          if self.attempt_save().await.is_err() {
            break;
          }
        },
        _collab = destroy_group_rx.recv() => {
          self.force_save().await;
          break;
        }
      }
    }
  }

  async fn force_save(&self) {
    if self.edit_state.is_new() && self.save(true).await.is_ok() {
      self.edit_state.set_is_new(false);
      return;
    }

    if !self.edit_state.is_edit() {
      trace!("skip force save collab to disk: {}", self.object_id);
      return;
    }

    if let Err(err) = self.save(false).await {
      warn!("fail to force save: {}:{:?}", self.object_id, err);
    }
  }

  /// return true if the collab has been dropped. Otherwise, return false
  async fn attempt_save(&self) -> Result<(), AppError> {
    trace!("collab:{} edit state: {}", self.object_id, self.edit_state);

    // Check if conditions for saving to disk are not met
    let is_new = self.edit_state.is_new();
    if self.edit_state.should_save_to_disk() {
      match self.save(is_new).await {
        Ok(_) => {
          if is_new {
            self.edit_state.set_is_new(false);
          }
        },
        Err(err) => {
          warn!("fail to write: {}:{}", self.object_id, err);
        },
      }
    }
    Ok(())
  }

  async fn save(&self, write_immediately: bool) -> Result<(), AppError> {
    let object_id = self.object_id.clone();
    let workspace_id = self.workspace_id.clone();
    let collab_type = self.collab_type.clone();
    let collab = match self.collab.upgrade() {
      Some(collab) => collab,
      None => return Err(AppError::Internal(anyhow!("collab has been dropped"))),
    };

    let params = {
      let cloned_collab = collab.clone();
      let (workspace_id, mut params, object_id) = tokio::task::spawn_blocking(move || {
        let collab = cloned_collab.blocking_read();
        let params = get_encode_collab(&workspace_id, &object_id, &collab, &collab_type)?;
        Ok::<_, AppError>((workspace_id, params, object_id))
      })
      .await??;

      let lock = collab.read().await;
      if let Some(indexer) = &self.indexer {
        match indexer.embedding_params(&lock) {
          Ok(embedding_params) => {
            drop(lock); // we no longer need the lock
            match indexer.embeddings(embedding_params).await {
              Ok(embeddings) => {
                params.embeddings = embeddings;
              },
              Err(err) => {
                warn!(
                  "failed to index embeddings from remote service for document {}/{}: {}",
                  workspace_id, object_id, err
                );
              },
            }
          },
          Err(err) => {
            warn!(
              "failed to get embedding params for document {}/{}: {}",
              workspace_id, object_id, err
            );
          },
        }
      }
      params
    };

    self
      .storage
      .insert_or_update_collab(&self.workspace_id, &self.uid, params, write_immediately)
      .await?;
    // Update the edit state on successful save
    self.edit_state.tick();
    Ok(())
  }
}

/// Encodes collaboration parameters for a given workspace and object.
///
/// This function attempts to encode collaboration details into a byte format based on the collaboration type.
/// It validates required data for the collaboration type before encoding.
/// If the collaboration type is `Folder`, it additionally checks for a workspace ID match.
///
#[inline]
fn get_encode_collab(
  workspace_id: &str,
  object_id: &str,
  collab: &Collab,
  collab_type: &CollabType,
) -> Result<CollabParams, AppError> {
  // Attempt to encode collaboration data to version 1 bytes and validate required data.
  let encoded_collab = collab
    .encode_collab_v1(|c| collab_type.validate_require_data(c))
    .map_err(|err| {
      AppError::Internal(anyhow!(
        "Failed to encode collaboration to bytes: {:?}",
        err
      ))
    })?
    .encode_to_bytes()
    .map_err(|err| {
      AppError::Internal(anyhow!(
        "Failed to serialize encoded collaboration to bytes: {:?}",
        err
      ))
    })?;

  // Specific check for collaboration type 'Folder' to ensure workspace ID consistency.
  if let CollabType::Folder = collab_type {
    validate_data_for_folder(collab, workspace_id)
      .map_err(|err| AppError::OverrideWithIncorrectData(err.to_string()))?;
  }

  // Construct and return collaboration parameters.
  let params = CollabParams {
    object_id: object_id.to_string(),
    encoded_collab_v1: encoded_collab,
    collab_type: collab_type.clone(),
    embeddings: None,
  };
  Ok(params)
}
