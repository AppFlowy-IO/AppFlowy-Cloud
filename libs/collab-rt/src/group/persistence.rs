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
use tracing::{info, trace, warn};

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
          if self.attempt_collab_save().await.is_err() {
            break;
          }
        },
        collab = destroy_group_rx.recv() => {
          if let Some(collab) = collab {
            self.force_save(collab).await;
          }
          break;
        }
      }
    }
  }

  async fn force_save(&self, collab: MutexCollab) {
    if !self.edit_state.is_edit() {
      trace!("skip force save collab to disk: {}", self.object_id);
      return;
    }

    let result = get_encode_collab(&self.object_id, &collab.lock(), &self.collab_type);
    match result {
      Ok(params) => {
        info!("[realtime] force save collab to disk: {}", self.object_id);
        match self
          .storage
          .insert_or_update_collab(&self.workspace_id, &self.uid, params)
          .await
        {
          Ok(_) => self.edit_state.tick(), // Update the edit state on successful save
          Err(err) => warn!("fail to force save collab to disk: {:?}", err),
        }
      },
      Err(err) => {
        warn!("fail to encode collab {}=>{:?}", self.object_id, err);
      },
    }
  }

  /// return true if the collab has been dropped. Otherwise, return false
  async fn attempt_collab_save(&self) -> Result<(), AppError> {
    // Check if conditions for saving to disk are not met
    if !self.edit_state.should_save_to_disk() {
      trace!("skip save collab to disk: {}", self.object_id);
      return Ok(());
    }

    let collab = match self.collab.upgrade() {
      Some(collab) => collab,
      None => return Err(AppError::Internal(anyhow!("collab has been dropped"))),
    };

    // Attempt to lock the collab; skip saving if unable
    let result = {
      match collab.try_lock() {
        Some(lock) => get_encode_collab(&self.object_id, &lock, &self.collab_type),
        None => return Ok(()),
      }
    };

    match result {
      Ok(params) => {
        info!("[realtime] save collab to disk: {}", self.object_id);
        match self
          .storage
          .insert_or_update_collab(&self.workspace_id, &self.uid, params)
          .await
        {
          Ok(_) => self.edit_state.tick(), // Update the edit state on successful save
          Err(err) => warn!("fail to save collab to disk: {:?}", err),
        }
      },
      Err(err) => {
        warn!("attempt to encode collab {}=>{:?}", self.object_id, err);
      },
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
