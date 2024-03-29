use crate::collaborate::group::EditState;
use crate::data_validation::validate_collab;
use anyhow::anyhow;
use app_error::AppError;
use collab::preclude::Collab;
use collab_entity::CollabType;
use database::collab::CollabStorage;
use database_entity::dto::CollabParams;
use std::ops::Deref;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::interval;
use tracing::{error, info};

pub(crate) struct GroupPersistence<S> {
  workspace_id: String,
  object_id: String,
  storage: Arc<S>,
  uid: i64,
  edit_state: Rc<EditState>,
  collab: Rc<Mutex<Collab>>,
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
    collab: Rc<Mutex<Collab>>,
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

  pub async fn run(self) {
    let mut interval = interval(Duration::from_secs(3 * 60));
    loop {
      interval.tick().await;

      if self.edit_state.should_save_to_disk(100, 3 * 60) {
        let result = if let Ok(lock_guard) = self.collab.try_lock() {
          get_encode_collab(&self.object_id, lock_guard.deref(), &self.collab_type)
        } else {
          continue;
        };

        if let Ok(params) = result {
          info!("[realtime] save collab to disk: {}", self.object_id);
          match self
            .storage
            .insert_or_update_collab(&self.workspace_id, &self.uid, params)
            .await
            .map_err(|err| {
              error!("fail to create new collab in plugin: {:?}", err);
              err
            }) {
            Ok(_) => {
              self.edit_state.tick();
            },
            Err(err) => {
              error!("fail to save collab to disk: {:?}", err)
            },
          }
        }
      }
    }
  }
}

#[inline]
fn get_encode_collab(
  object_id: &str,
  collab: &Collab,
  collab_type: &CollabType,
) -> Result<CollabParams, AppError> {
  validate_collab(collab, collab_type).map_err(|err| AppError::NoRequiredData(err.to_string()))?;

  let result = collab
    .try_encode_collab_v1()
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
