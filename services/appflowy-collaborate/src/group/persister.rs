use crate::error::RealtimeError;
use crate::indexer::Indexer;
use crate::CollabRealtimeMetrics;
use app_error::AppError;
use collab::core::collab::DataSource;
use collab::core::origin::CollabOrigin;
use collab::entity::EncodedCollab;
use collab::preclude::Collab;
use collab_entity::CollabType;
use collab_rt_protocol::RTProtocolError;
use collab_stream::client::CollabRedisStream;
use collab_stream::collab_update_sink::{AwarenessUpdateSink, CollabUpdateSink};
use collab_stream::error::StreamError;
use collab_stream::model::{AwarenessStreamUpdate, CollabStreamUpdate, MessageId, UpdateFlags};
use database::collab::{CollabStorage, GetCollabOrigin};
use database_entity::dto::{
  AFCollabEmbeddings, CollabParams, InsertSnapshotParams, QueryCollabParams,
};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};
use yrs::updates::encoder::Encode;
use yrs::{ReadTxn, StateVector, Update};

pub struct CollabPersister {
  uid: i64,
  storage: Arc<dyn CollabStorage>,
  indexer: Option<Arc<dyn Indexer>>,
  metrics: Arc<CollabRealtimeMetrics>,
  /// A grace period for prunning Redis collab updates. Instead of deleting all messages we
  /// read right away, we give 1min for other potential client to catch up.
  prune_grace_period: Duration,
  pub collab_redis_stream: Arc<CollabRedisStream>,
}

impl CollabPersister {
  #[allow(clippy::too_many_arguments)]
  pub fn new(
    uid: i64,
    storage: Arc<dyn CollabStorage>,
    collab_redis_stream: Arc<CollabRedisStream>,
    indexer: Option<Arc<dyn Indexer>>,
    metrics: Arc<CollabRealtimeMetrics>,
    prune_grace_period: Duration,
  ) -> Self {
    Self {
      uid,
      storage,
      collab_redis_stream,
      indexer,
      metrics,
      prune_grace_period,
    }
  }

  /// Loads collab without its history. Used for handling y-sync protocol messages.
  pub async fn load_compact(
    &self,
    workspace_id: &str,
    object_id: &str,
    collab_type: CollabType,
  ) -> Result<CollabSnapshot, RealtimeError> {
    // 1. Try to load the latest snapshot from storage
    let start = Instant::now();
    let mut collab = match self
      .load_collab_full(workspace_id, object_id, collab_type, false)
      .await?
    {
      Some(collab) => collab,
      None => Collab::new_with_origin(CollabOrigin::Server, object_id, vec![], false),
    };
    self.metrics.load_collab_count.inc();

    // 2. consume all Redis updates on top of it (keep redis msg id)
    let mut last_message_id = None;
    let mut tx = collab.transact_mut();
    let updates = self
      .collab_redis_stream
      .current_collab_updates(workspace_id, object_id, None)  //TODO: store Redis last msg id somewhere in doc state snapshot and replay from there
      .await?;
    let mut i = 0;
    for (message_id, update) in updates {
      i += 1;
      let update: Update = update.into_update()?;
      tx.apply_update(update)
        .map_err(|err| RTProtocolError::YrsApplyUpdate(err.to_string()))?;
      last_message_id = Some(message_id); //TODO: shouldn't this happen before decoding?
      self.metrics.apply_update_count.inc();
    }
    drop(tx);
    tracing::trace!(
      "loaded collab compact state: {} replaying {} updates",
      object_id,
      i
    );
    self
      .metrics
      .load_collab_time
      .observe(start.elapsed().as_millis() as f64);

    // now we have the most recent version of the document
    let snapshot = CollabSnapshot {
      collab,
      last_message_id,
    };
    Ok(snapshot)
  }

  /// Returns a collab state (with GC turned off), but only if there were any pending updates
  /// waiting to be merged into main document state.
  async fn load_if_changed(
    &self,
    workspace_id: &str,
    object_id: &str,
    collab_type: CollabType,
  ) -> Result<Option<CollabSnapshot>, RealtimeError> {
    // 1. load pending Redis updates
    let updates = self
      .collab_redis_stream
      .current_collab_updates(workspace_id, object_id, None)
      .await?;

    let start = Instant::now();
    let mut i = 0;
    let mut collab = None;
    let mut last_message_id = None;
    for (message_id, update) in updates {
      i += 1;
      let update: Update = update.into_update()?;
      if collab.is_none() {
        collab = Some(
          match self
            .load_collab_full(workspace_id, object_id, collab_type.clone(), true)
            .await?
          {
            Some(collab) => collab,
            None => Collab::new_with_origin(CollabOrigin::Server, object_id, vec![], true),
          },
        )
      };
      let collab = collab.as_mut().unwrap();
      collab
        .transact_mut()
        .apply_update(update)
        .map_err(|err| RTProtocolError::YrsApplyUpdate(err.to_string()))?;
      last_message_id = Some(message_id); //TODO: shouldn't this happen before decoding?
      self.metrics.apply_update_count.inc();
    }

    // if there were no Redis updates, collab is still not initialized
    match collab {
      Some(collab) => {
        self.metrics.load_full_collab_count.inc();
        let elapsed = start.elapsed();
        self
          .metrics
          .load_collab_time
          .observe(elapsed.as_millis() as f64);
        tracing::trace!(
          "loaded collab full state: {} replaying {} updates in {:?}",
          object_id,
          i,
          elapsed
        );
        {
          let tx = collab.transact();
          if tx.store().pending_update().is_some() || tx.store().pending_ds().is_some() {
            tracing::trace!(
              "loaded collab {} is incomplete: has pending data",
              object_id
            );
          }
        }
        Ok(Some(CollabSnapshot {
          collab,
          last_message_id,
        }))
      },
      None => Ok(None),
    }
  }

  pub async fn save_database(
    &self,
    workspace_id: &str,
    database_id: &str,
  ) -> Result<(), RealtimeError> {
    todo!()
  }

  pub async fn save(
    &self,
    workspace_id: &str,
    object_id: &str,
    collab_type: CollabType,
  ) -> Result<(), RealtimeError> {
    // load collab but only if there were pending updates in Redis
    if let Some(mut snapshot) = self
      .load_if_changed(workspace_id, object_id, collab_type.clone())
      .await?
    {
      tracing::debug!("requesting save for collab {}", object_id);
      if let Some(message_id) = snapshot.last_message_id {
        // non-nil message_id means that we had to update the most recent collab state snapshot
        // with new updates from Redis. This means that our snapshot state is newer than the last
        // persisted one in the database
        self
          .save_attempt(workspace_id, &mut snapshot.collab, collab_type, message_id)
          .await?;
      }
    } else {
      tracing::trace!("collab {} state has not changed", object_id);
    }
    Ok(())
  }

  /// Tries to save provided `snapshot`. This snapshot is expected to have **GC turned off**, as
  /// first it will try to save it as a historical snapshot (will all updates available), then it
  /// will generate another (compact) snapshot variant that will be used as main one for loading
  /// for the sake of y-sync protocol.
  async fn save_attempt(
    &self,
    workspace_id: &str,
    collab: &mut Collab,
    collab_type: CollabType,
    message_id: MessageId,
  ) -> Result<(), RealtimeError> {
    let object_id = collab.object_id().to_owned();
    if !collab.get_awareness().doc().skip_gc() {
      return Err(RealtimeError::UnexpectedData(
        "tried to save history for snapshot with GC turned on",
      ));
    }
    // try to acquire snapshot lease - it's possible that multiple web services will try to
    // perform snapshot at the same time, so we'll use lease to let only one of them atm.
    if let Some(mut lease) = self
      .collab_redis_stream
      .lease(workspace_id, &object_id)
      .await?
    {
      // 1. Save full historic document state
      let mut tx = collab.transact_mut();
      let sv = tx.state_vector().encode_v1();
      let doc_state_full = tx.encode_state_as_update_v1(&StateVector::default());
      let full_len = doc_state_full.len();
      let encoded_collab = EncodedCollab::new_v1(sv.clone(), doc_state_full)
        .encode_to_bytes()
        .map_err(|err| RealtimeError::Internal(err.into()))?;
      self
        .metrics
        .full_collab_size
        .observe(encoded_collab.len() as f64);
      let params = InsertSnapshotParams {
        object_id: object_id.clone(),
        encoded_collab_v1: encoded_collab,
        workspace_id: workspace_id.into(),
        collab_type: collab_type.clone(),
      };
      self
        .storage
        .create_snapshot(params)
        .await
        .map_err(|err| RealtimeError::Internal(err.into()))?;

      // 2. Generate document state with GC turned on and save it.
      tx.force_gc();
      drop(tx);

      let doc_state_light = collab
        .transact()
        .encode_state_as_update_v1(&StateVector::default());
      let light_len = doc_state_light.len();
      let encoded_collab = EncodedCollab::new_v1(sv, doc_state_light)
        .encode_to_bytes()
        .map_err(|err| RealtimeError::Internal(err.into()))?;
      self
        .metrics
        .collab_size
        .observe(encoded_collab.len() as f64);
      let mut params = CollabParams::new(object_id.clone(), collab_type.clone(), encoded_collab);

      match self.embeddings(collab).await {
        Ok(embeddings) => params.embeddings = embeddings,
        Err(err) => tracing::warn!("failed to fetch embeddings `{}`: {}", object_id, err),
      }

      self
        .storage
        .queue_insert_or_update_collab(workspace_id, &self.uid, params, true)
        .await
        .map_err(|err| RealtimeError::Internal(err.into()))?;

      tracing::debug!(
        "persisted collab {} snapshot at {}: {} and {} bytes",
        object_id,
        message_id,
        full_len,
        light_len
      );

      // 3. finally we can drop Redis messages
      let now = SystemTime::UNIX_EPOCH.elapsed().unwrap().as_millis();
      let msg_id = MessageId {
        timestamp_ms: (now - self.prune_grace_period.as_millis()) as u64,
        sequence_number: 0,
      };
      let stream_key = CollabStreamUpdate::stream_key(workspace_id, &object_id);
      self
        .collab_redis_stream
        .prune_stream(&stream_key, msg_id)
        .await?;

      let _ = lease.release().await;
    }

    Ok(())
  }

  async fn embeddings(&self, collab: &Collab) -> Result<Option<AFCollabEmbeddings>, AppError> {
    if let Some(indexer) = self.indexer.clone() {
      let params = indexer.embedding_params(collab).await?;
      let embeddings = indexer.embeddings(params).await?;
      Ok(embeddings)
    } else {
      Ok(None)
    }
  }

  async fn load_collab_full(
    &self,
    workspace_id: &str,
    object_id: &str,
    collab_type: CollabType,
    keep_history: bool,
  ) -> Result<Option<Collab>, RealtimeError> {
    let doc_state = if keep_history {
      // if we want history-keeping variant, we need to get a snapshot
      let snapshot = self
        .storage
        .get_latest_snapshot(workspace_id, object_id)
        .await
        .map_err(|err| RealtimeError::Internal(err.into()))?;
      match snapshot {
        None => None,
        Some(snapshot) => {
          let encoded_collab = EncodedCollab::decode_from_bytes(&snapshot.encoded_collab_v1)
            .map_err(|err| RealtimeError::Internal(err.into()))?;
          Some(encoded_collab.doc_state)
        },
      }
    } else {
      None // if we want a lightweight variant, we'll fallback to default
    };
    let doc_state = match doc_state {
      Some(doc_state) => doc_state,
      None => {
        // we didn't find a snapshot, or we want a lightweight collab version
        let params =
          QueryCollabParams::new(object_id.clone(), collab_type.clone(), workspace_id.clone());
        let result = self
          .storage
          .get_encode_collab(GetCollabOrigin::Server, params, false)
          .await;
        match result {
          Ok(encoded_collab) => encoded_collab.doc_state,
          Err(AppError::RecordNotFound(_)) => return Ok(None),
          Err(err) => return Err(RealtimeError::Internal(err.into())),
        }
      },
    };

    let collab: Collab = Collab::new_with_source(
      CollabOrigin::Server,
      object_id,
      DataSource::DocStateV1(doc_state.into()),
      vec![],
      keep_history, // should we use history-remembering version (true) or lightweight one (false)?
    )?;
    Ok(Some(collab))
  }
}

pub struct CollabSnapshot {
  pub collab: Collab,
  pub last_message_id: Option<MessageId>,
}
