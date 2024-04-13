use crate::group::group_init::{EditState, MutexCollab, WeakMutexCollab};

use anyhow::anyhow;
use app_error::AppError;
use collab::preclude::Collab;
use collab_entity::CollabType;
use database::collab::CollabStorage;
use database_entity::dto::CollabParams;

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::{interval, sleep};
use tracing::{trace, warn};

pub(crate) struct GroupPersistence<S> {
  workspace_id: String,
  object_id: String,
  storage: Arc<S>,
  uid: i64,
  edit_state: Arc<EditState>,
  collab: WeakMutexCollab,
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
    collab: WeakMutexCollab,
    collab_type: CollabType,
  ) -> Self {
    Self {
      workspace_id,
      object_id,
      uid,
      storage,
      edit_state,
      collab,
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
    if !self.edit_state.is_edit() {
      trace!("skip force save collab to disk: {}", self.object_id);
      return;
    }
    if let Err(err) = self.save().await {
      warn!("fail to force save: {}:{:?}", self.object_id, err);
    }
  }

  /// return true if the collab has been dropped. Otherwise, return false
  async fn attempt_save(&self) -> Result<(), AppError> {
    // Check if conditions for saving to disk are not met
    if !self.edit_state.should_save_to_disk() {
      trace!("skip save collab to disk: {}", self.object_id);
      return Ok(());
    }

    self.save().await?;
    Ok(())
  }

  async fn save(&self) -> Result<(), AppError> {
    let mutex_collab = self.collab.clone();
    let object_id = self.object_id.clone();
    let collab_type = self.collab_type.clone();
    let collab = match mutex_collab.upgrade() {
      Some(collab) => collab,
      None => return Err(AppError::Internal(anyhow!("collab has been dropped"))),
    };

    let result = tokio::task::spawn_blocking(move || {
      // Attempt to lock the collab; skip saving if unable
      let lock_guard = collab.try_lock()?;
      let params = get_encode_collab(&object_id, &lock_guard, &collab_type).ok()?;
      Some(params)
    })
    .await;

    match result {
      Ok(Some(params)) => {
        match self
          .storage
          .insert_or_update_collab(&self.workspace_id, &self.uid, params)
          .await
        {
          Ok(_) => {
            trace!("[realtime] save collab to disk: {}", self.object_id);
            // Update the edit state on successful save
            self.edit_state.tick();
          },
          Err(err) => warn!("fail to save collab to disk: {:?}", err),
        }
      },
      Ok(None) => {
        // required lock failed or get encode collab failed, skip saving
      },
      Err(err) => warn!("attempt to encode collab {}=>{:?}", self.object_id, err),
    }
    Ok(())
  }
}

#[inline]
fn get_encode_collab(
  object_id: &str,
  collab: &Collab,
  collab_type: &CollabType,
) -> Result<CollabParams, AppError> {
  let result = collab
    .try_encode_collab_v1(|collab| collab_type.validate(collab))
    .map_err(|err| AppError::Internal(anyhow!("fail to encode collab to bytes: {:?}", err)))?
    .encode_to_bytes();

  match result {
    Ok(encoded_collab_v1) => {
      let params = CollabParams {
        object_id: object_id.to_string(),
        encoded_collab_v1,
        collab_type: collab_type.clone(),
      };

      Ok(params)
    },
    Err(err) => Err(AppError::Internal(anyhow!(
      "fail to encode doc to bytes: {:?}",
      err
    ))),
  }
}
