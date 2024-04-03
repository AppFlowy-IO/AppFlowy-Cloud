use crate::collaborate::group::EditState;

use anyhow::anyhow;
use app_error::AppError;
use collab::preclude::Collab;
use collab_entity::CollabType;
use database::collab::CollabStorage;
use database_entity::dto::CollabParams;

use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, Mutex};
use tokio::time::{interval, sleep};
use tracing::{info, warn};

pub(crate) struct GroupPersistence<S> {
  workspace_id: String,
  object_id: String,
  storage: Arc<S>,
  uid: i64,
  edit_state: Rc<EditState>,
  collab: Weak<Mutex<Collab>>,
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
    edit_state: Rc<EditState>,
    collab: Weak<Mutex<Collab>>,
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

  pub async fn run(self, mut destroy_group_rx: mpsc::Receiver<Rc<Mutex<Collab>>>) {
    // defer start saving after 5 seconds
    sleep(Duration::from_secs(5)).await;
    let mut interval = interval(Duration::from_secs(180)); // 3 minutes

    loop {
      tokio::select! {
        _ = interval.tick() => {
          if self.attempt_collab_save().await {
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

  async fn force_save(&self, collab: Rc<Mutex<Collab>>) {
    let lock_guard = collab.lock().await;
    let result = get_encode_collab(&self.object_id, &lock_guard, &self.collab_type);
    drop(lock_guard);

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
        warn!("fail to encode collab {} error:{:?}", self.object_id, err);
      },
    }
  }

  /// return true if the collab has been dropped. Otherwise, return false
  async fn attempt_collab_save(&self) -> bool {
    let collab = match self.collab.upgrade() {
      Some(collab) => collab,
      None => return true, // End the loop if the collab has been dropped
    };

    // Check if conditions for saving to disk are not met
    if !self.edit_state.should_save_to_disk() {
      // 100 edits or 1 hour
      return false;
    }

    // Attempt to lock the collab; skip saving if unable
    let lock_guard = match collab.try_lock() {
      Ok(lock) => lock,
      Err(_) => return false,
    };
    let result = get_encode_collab(&self.object_id, &lock_guard, &self.collab_type);
    drop(lock_guard);

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
        warn!(
          "attempt to encode collab {} error:{:?}",
          self.object_id, err
        );
      },
    }

    // Continue the loop
    false
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
        override_if_exist: false,
      };

      Ok(params)
    },
    Err(err) => Err(AppError::Internal(anyhow!(
      "fail to encode doc to bytes: {:?}",
      err
    ))),
  }
}
