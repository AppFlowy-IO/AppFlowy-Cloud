use std::time::{Duration, Instant};

use collab::core::collab::{DataSource, MutexCollab, TransactionMutExt, WeakMutexCollab};
use collab::core::origin::CollabOrigin;
use collab::preclude::updates::decoder::Decode;
use collab::preclude::{Collab, Update};
use collab_entity::CollabType;
use tokio::task::JoinHandle;
use tokio::time::interval;
use tracing::instrument;
use yrs::types::ToJson;
use yrs::{ReadTxn, Transact};

use appflowy_ai_client::client::AppFlowyAIClient;
use appflowy_ai_client::dto::Document;
use collab_stream::client::CollabRedisStream;
use collab_stream::model::{CollabUpdateEvent, StreamMessage};
use collab_stream::stream_group::{ReadOption, StreamGroup};

use crate::error::Result;

const CONSUMER_NAME: &str = "open_collab_handle";

#[allow(dead_code)]
pub struct CollabHandle {
  collab: MutexCollab,
  task: JoinHandle<()>,
}

impl CollabHandle {
  pub(crate) async fn open(
    redis_stream: &CollabRedisStream,
    ai_client: AppFlowyAIClient,
    object_id: String,
    workspace_id: String,
    collab_type: CollabType,
    doc_state: Vec<u8>,
    ingest_interval: Duration,
  ) -> Result<Self> {
    let group_name = format!("indexer_{}:{}", workspace_id, object_id);
    let mut update_stream = redis_stream
      .collab_update_stream(&workspace_id, &object_id, &group_name)
      .await
      .unwrap();

    let messages = update_stream.get_unacked_messages(CONSUMER_NAME).await?;
    let collab = {
      let mut collab = Collab::new_with_source(
        CollabOrigin::Empty,
        &object_id,
        DataSource::DocStateV1(doc_state),
        vec![],
        false,
      )?;
      collab.initialize();
      MutexCollab::new(collab)
    };
    if !messages.is_empty() {
      if Self::handle_collab_updates(&mut update_stream, &collab, messages).await? {
        // for first access (which may have happened after ie. server restart) if we detected any
        // changes, we try to update index immediately
        Self::handle_index_event(
          &ai_client,
          &collab,
          object_id.clone(),
          workspace_id.clone(),
          &collab_type,
        )
        .await?;
      }
    }

    let task = tokio::spawn(Self::receive_collab_updates(
      update_stream,
      ai_client,
      collab.downgrade(),
      object_id,
      workspace_id,
      collab_type,
      ingest_interval,
    ));

    Ok(Self { collab, task })
  }

  /// In regular time intervals, receive yrs updates and apply them to the locall in-memory collab
  /// representation. This should emit index content events, which we listen to on
  /// [Self::receive_index_events].
  async fn receive_collab_updates(
    mut update_stream: StreamGroup,
    ai_client: AppFlowyAIClient,
    collab: WeakMutexCollab,
    object_id: String,
    workspace_id: String,
    collab_type: CollabType,
    ingest_interval: Duration,
  ) {
    let mut interval = interval(ingest_interval);
    let mut last_updated = Instant::now();
    let mut has_changed = false;
    loop {
      interval.tick().await;
      let result = update_stream
        .consumer_messages(CONSUMER_NAME, ReadOption::Count(100))
        .await;
      match result {
        Ok(messages) => {
          if let Some(collab) = collab.upgrade() {
            // check if we received empty message batch, if not: update the collab
            if !messages.is_empty() {
              match Self::handle_collab_updates(&mut update_stream, &collab, messages).await {
                Ok(changed) => has_changed |= changed,
                Err(err) => tracing::error!("failed to handle messages: {}", err),
              }
            }

            // if we received any changes since the last search index update within the time frame
            // we should re-index the collab
            let now = Instant::now();
            if has_changed && now - last_updated >= ingest_interval {
              if let Err(err) = Self::handle_index_event(
                &ai_client,
                &collab,
                object_id.clone(),
                workspace_id.clone(),
                &collab_type,
              )
              .await
              {
                tracing::error!("failed to send index event to appflowy AI: {}", err);
              } else {
                tracing::trace!(
                  "successfully updated collab index {}/{}",
                  workspace_id,
                  object_id
                );
                last_updated = now;
                has_changed = false;
              }
            }
          } else {
            tracing::trace!("collab dropped, stopping consumer");
            return;
          }
        },
        Err(err) => {
          tracing::error!("failed to receive messages: {}", err);
        },
      }
    }
  }

  #[instrument(skip(update_stream, collab, messages), fields(messages = messages.len()))]
  async fn handle_collab_updates(
    update_stream: &mut StreamGroup,
    collab: &MutexCollab,
    messages: Vec<StreamMessage>,
  ) -> Result<bool> {
    let mut has_changed = false;
    if let Some(collab) = collab.try_lock() {
      let mut txn = collab.try_transaction_mut()?;

      for message in &messages {
        if let Ok(event) = CollabUpdateEvent::decode(&message.data) {
          match event {
            CollabUpdateEvent::UpdateV1 { encode_update } => {
              let update = Update::decode_v1(&encode_update)?;
              txn.try_apply_update(update)?;
            },
          }
        }
      }
      txn.commit();
      has_changed = txn.before_state() != txn.after_state();
    } else {
      tracing::error!("failed to lock collab");
    }
    update_stream.ack_messages(&messages).await?;

    Ok(has_changed)
  }

  /// Convert the collab type to the AI client's collab type.
  /// Return `None` if indexing of a documents of given type is not supported - it will not cause
  /// an error, but will be logged as a warning.
  fn map_collab(
    collab_entity: &CollabType,
    collab: &MutexCollab,
  ) -> Option<(appflowy_ai_client::dto::CollabType, String)> {
    match collab_entity {
      CollabType::Document => {
        if let Some(collab) = collab.try_lock() {
          let doc = collab.get_doc();
          let txn = doc.transact();
          if let Some(data_ref) = txn.get_map("data") {
            let data = data_ref.to_json(&txn).to_string();
            return Some((appflowy_ai_client::dto::CollabType::Document, data));
          }
        }
        None
      },
      _ => None,
    }
  }

  #[instrument(skip(client, collab))]
  async fn handle_index_event(
    client: &AppFlowyAIClient,
    collab: &MutexCollab,
    object_id: String,
    workspace_id: String,
    collab_type: &CollabType,
  ) -> Result<()> {
    if let Some((collab_type, json)) = Self::map_collab(collab_type, collab) {
      let document = Document {
        id: object_id,
        doc_type: collab_type,
        workspace_id,
        content: json,
      };
      client.index_documents(&[document]).await?;
    }
    Ok(())
  }
}

impl Drop for CollabHandle {
  fn drop(&mut self) {
    self.task.abort()
  }
}

#[cfg(test)]
mod test {
  use std::time::Duration;

  use collab::core::transaction::DocTransactionExtension;
  use collab::preclude::Collab;
  use collab_entity::CollabType;
  use yrs::{Map, Subscription};

  use appflowy_ai_client::client::AppFlowyAIClient;
  use appflowy_ai_client::dto::SearchDocumentsRequest;
  use collab_stream::client::CollabRedisStream;
  use collab_stream::model::CollabUpdateEvent;
  use collab_stream::stream_group::StreamGroup;

  use crate::collab_handle::CollabHandle;

  #[tokio::test]
  async fn test_indexing_pipeline() {
    let _ = env_logger::builder().is_test(true).try_init();

    let redis_stream = redis_stream().await;
    let workspace_id = uuid::Uuid::new_v4().to_string();
    let object_id = uuid::Uuid::new_v4().to_string();

    let mut collab = Collab::new(1, object_id.clone(), "device-1".to_string(), vec![], false);
    collab.initialize();
    let ai_client = AppFlowyAIClient::new("http://localhost:5001");

    let stream_group = redis_stream
      .collab_update_stream(&workspace_id, &object_id, "indexer")
      .await
      .unwrap();

    let _s = collab_update_forwarder(&mut collab, stream_group.clone());
    let doc = collab.get_doc();
    let init_state = doc.get_encoded_collab_v1().doc_state;
    let views_ref = doc.get_or_insert_map("views");
    collab.with_origin_transact_mut(|txn| {
      views_ref.insert(txn, "key", "value");
    });

    let handle = CollabHandle::open(
      &redis_stream,
      ai_client.clone(),
      object_id.clone(),
      workspace_id.clone(),
      CollabType::Document,
      init_state.into(),
      Duration::from_millis(50),
    )
    .await
    .unwrap();

    tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;

    let docs = ai_client
      .search_documents(&SearchDocumentsRequest {
        workspaces: vec![workspace_id],
        query: "key".to_string(),
        result_count: None,
      })
      .await
      .unwrap();

    assert_eq!(docs.len(), 1);
  }

  pub async fn redis_client() -> redis::Client {
    let redis_uri = "redis://localhost:6379";
    redis::Client::open(redis_uri).expect("failed to connect to redis")
  }

  pub async fn redis_stream() -> CollabRedisStream {
    let redis_client = redis_client().await;
    CollabRedisStream::new(redis_client)
      .await
      .expect("failed to create stream client")
  }

  pub fn collab_update_forwarder(collab: &mut Collab, mut stream: StreamGroup) -> Subscription {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    tokio::spawn(async move {
      while let Some(data) = rx.recv().await {
        println!("sending update to redis");
        stream.insert_message(data).await.unwrap();
      }
    });
    collab
      .get_doc()
      .observe_update_v1(move |_, e| {
        println!("Observed update");
        let e = CollabUpdateEvent::UpdateV1 {
          encode_update: e.update.clone(),
        };
        tx.send(e).unwrap();
      })
      .unwrap()
  }
}
