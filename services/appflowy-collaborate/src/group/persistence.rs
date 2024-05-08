use crate::group::group_init::EditState;

use anyhow::anyhow;
use app_error::AppError;
use collab::preclude::Collab;
use collab_entity::{validate_data_for_folder, CollabType};
use database::collab::CollabStorage;
use database_entity::dto::CollabParams;

use collab::core::collab::{MutexCollab, WeakMutexCollab};

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::{interval, sleep};
use tracing::{error, trace, warn};

pub(crate) struct GroupPersistence<S> {
  workspace_id: String,
  object_id: String,
  storage: Arc<S>,
  uid: i64,
  edit_state: Arc<EditState>,
  mutex_collab: WeakMutexCollab,
  collab_type: CollabType,
}

impl<S> GroupPersistence<S>
where
  S: CollabStorage,
{
  pub fn new(
    workspace_id: String,
    object_id: String,
    uid: i64,
    storage: Arc<S>,
    edit_state: Arc<EditState>,
    mutex_collab: WeakMutexCollab,
    collab_type: CollabType,
  ) -> Self {
    Self {
      workspace_id,
      object_id,
      uid,
      storage,
      edit_state,
      mutex_collab,
      collab_type,
    }
  }

  pub async fn run(self, mut destroy_group_rx: mpsc::Receiver<MutexCollab>) {
    let mut interval = interval(Duration::from_secs(60));
    // TODO(nathan): remove this sleep when creating a new collab, applying all the updates
    // workarounds for the issue that the collab doesn't contain the required data when first created
    sleep(Duration::from_secs(5)).await;
    loop {
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
    if self.edit_state.is_new() && self.save(true).await.is_ok() {
      self.edit_state.set_is_new(false);
      return Ok(());
    }

    // Check if conditions for saving to disk are not met
    if !self.edit_state.should_save_to_disk() {
      return Ok(());
    }
    self.save(false).await?;
    Ok(())
  }

  async fn save(&self, write_immediately: bool) -> Result<(), AppError> {
    let mutex_collab = self.mutex_collab.clone();
    let object_id = self.object_id.clone();
    let workspace_id = self.workspace_id.clone();
    let collab_type = self.collab_type.clone();
    let collab = match mutex_collab.upgrade() {
      Some(collab) => collab,
      None => return Err(AppError::Internal(anyhow!("collab has been dropped"))),
    };

    let result = tokio::task::spawn_blocking(move || {
      // Attempt to lock the collab; skip saving if unable
      let lock_guard = collab
        .try_lock()
        .ok_or_else(|| AppError::Internal(anyhow!("required lock failed")))?;
      let params = get_encode_collab(&workspace_id, &object_id, &lock_guard, &collab_type)?;
      Ok::<_, AppError>(params)
    })
    .await;

    match result {
      Ok(Ok(params)) => {
        match self
          .storage
          .insert_or_update_collab(&self.workspace_id, &self.uid, params, write_immediately)
          .await
        {
          Ok(_) => {
            // Update the edit state on successful save
            self.edit_state.tick();
          },
          Err(err) => warn!("fail to save collab to disk: {:?}", err),
        }
      },
      Ok(Err(err)) => {
        if matches!(err, AppError::OverrideWithIncorrectData(_)) {
          return Err(err);
        }
        // omits the other errors
      },
      Err(err) => {
        if err.is_panic() {
          // reason:
          // 1. Couldn't get item's parent
          warn!(
            "encode collab panic:{}:{}=>{:?}",
            self.object_id, self.collab_type, err
          );
        } else {
          error!("fail to spawn a task to get encode collab: {:?}", err)
        }
      },
    }
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
    .try_encode_collab_v1(|c| collab_type.validate_require_data(c))
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
  };
  Ok(params)
}
