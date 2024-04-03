use crate::error::RealtimeError;
use crate::RealtimeAccessControl;
use app_error::AppError;
use async_trait::async_trait;
use collab::core::collab::{DocStateSource, TransactionMutExt};
use collab::core::collab_plugin::EncodedCollab;
use collab::core::origin::CollabOrigin;
use collab::preclude::{Collab, CollabPlugin, Doc};
use collab_entity::CollabType;
use database::collab::CollabStorage;
use database_entity::dto::{InsertSnapshotParams, QueryCollabParams};
use std::sync::Arc;
use tracing::{error, event, info, span, Instrument, Level};
use yrs::updates::decoder::Decode;
use yrs::{Transact, Update};

pub struct LoadCollabPlugin<S, AC> {
  uid: i64,
  workspace_id: String,
  storage: Arc<S>,
  collab_type: CollabType,
  #[allow(dead_code)]
  access_control: Arc<AC>,
}

impl<S, AC> LoadCollabPlugin<S, AC>
where
  S: CollabStorage,
  AC: RealtimeAccessControl,
{
  pub fn new(
    uid: i64,
    workspace_id: &str,
    collab_type: CollabType,
    storage: S,
    access_control: Arc<AC>,
  ) -> Self {
    let storage = Arc::new(storage);
    let workspace_id = workspace_id.to_string();
    Self {
      uid,
      workspace_id,
      storage,
      collab_type,
      access_control,
    }
  }
}

async fn init_collab(
  oid: &str,
  encoded_collab: &EncodedCollab,
  doc: &Doc,
) -> Result<(), RealtimeError> {
  if encoded_collab.doc_state.is_empty() {
    return Err(RealtimeError::UnexpectedData("doc state is empty"));
  }

  // Can turn off INFO level into DEBUG. For now, just to see the log
  event!(
    Level::DEBUG,
    "start decoding:{} state len: {}, sv len: {}, v: {:?}",
    oid,
    encoded_collab.doc_state.len(),
    encoded_collab.state_vector.len(),
    encoded_collab.version
  );
  let update = Update::decode_v1(&encoded_collab.doc_state)?;
  let mut txn = doc.transact_mut();
  txn.try_apply_update(update)?;
  drop(txn);

  Ok(())
}

#[async_trait]
impl<S, AC> CollabPlugin for LoadCollabPlugin<S, AC>
where
  S: CollabStorage,
  AC: RealtimeAccessControl,
{
  async fn init(&self, object_id: &str, _origin: &CollabOrigin, doc: &Doc) {
    let params = QueryCollabParams::new(object_id, self.collab_type.clone(), &self.workspace_id);
    match self
      .storage
      .get_collab_encoded(&self.uid, params, true)
      .await
    {
      Ok(encoded_collab_v1) => match init_collab(object_id, &encoded_collab_v1, doc).await {
        Ok(_) => {
          // Attempt to create a snapshot for the collaboration object. When creating this snapshot, it is
          // assumed that the 'encoded_collab_v1' is already in a valid format. Therefore, there is no need
          // to verify the outcome of the 'encode_to_bytes' operation.
          if self.storage.should_create_snapshot(object_id).await {
            let cloned_workspace_id = self.workspace_id.clone();
            let cloned_object_id = object_id.to_string();
            let storage = self.storage.clone();
            let collab_type = self.collab_type.clone();
            event!(Level::DEBUG, "Creating collab snapshot");
            let _ = tokio::task::spawn_blocking(move || {
              let params = InsertSnapshotParams {
                object_id: cloned_object_id,
                encoded_collab_v1: encoded_collab_v1.encode_to_bytes().unwrap(),
                workspace_id: cloned_workspace_id,
                collab_type,
              };

              tokio::spawn(async move {
                if let Err(err) = storage.queue_snapshot(params).await {
                  error!("create snapshot {:?}", err);
                }
              });
            })
            .await;
          }
        },
        Err(err) => {
          let span = span!(Level::ERROR, "restore_collab_from_snapshot", object_id = %object_id, error = %err);
          async {
            match get_latest_snapshot(
              &self.workspace_id,
              object_id,
              &self.storage,
              &self.collab_type,
            )
            .await
            {
              None => error!("No snapshot found for collab"),
              Some(encoded_collab) => match init_collab(object_id, &encoded_collab, doc).await {
                Ok(_) => info!("Restore collab with snapshot success"),
                Err(err) => {
                  error!("Restore collab with snapshot failed: {:?}", err);
                },
              },
            }
          }
          .instrument(span)
          .await;
        },
      },
      Err(err) => match &err {
        AppError::RecordNotFound(_) => {
          event!(
            Level::DEBUG,
            "Can't find the collab:{} from realtime editing",
            object_id
          );
        },
        _ => error!("{}", err),
      },
    }
  }
}

async fn get_latest_snapshot<S>(
  workspace_id: &str,
  object_id: &str,
  storage: &S,
  collab_type: &CollabType,
) -> Option<EncodedCollab>
where
  S: CollabStorage,
{
  let metas = storage.get_collab_snapshot_list(object_id).await.ok()?.0;
  for meta in metas {
    let snapshot_data = storage
      .get_collab_snapshot(workspace_id, &meta.object_id, &meta.snapshot_id)
      .await
      .ok()?;
    if let Ok(encoded_collab) = EncodedCollab::decode_from_bytes(&snapshot_data.encoded_collab_v1) {
      if let Ok(collab) = Collab::new_with_doc_state(
        CollabOrigin::Empty,
        object_id,
        DocStateSource::FromDocState(encoded_collab.doc_state.to_vec()),
        vec![],
        false,
      ) {
        // TODO(nathan): this check is not necessary, can be removed in the future.
        collab_type.validate(&collab).ok()?;
        return Some(encoded_collab);
      }
    }
  }
  None
}
