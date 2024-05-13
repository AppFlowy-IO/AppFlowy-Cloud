use std::time::{Duration, Instant};

use collab::core::collab::{
  DataSource, IndexContent, IndexContentReceiver, MutexCollab, TransactionMutExt, WeakMutexCollab,
};
use collab::core::origin::CollabOrigin;
use collab::preclude::updates::decoder::Decode;
use collab::preclude::{Collab, Update};
use collab_entity::CollabType;
use tokio::select;
use tokio::task::JoinSet;
use tokio::time::interval;
use tracing::instrument;

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
  tasks: JoinSet<()>,
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
    let (collab, index_content) = {
      let mut collab = Collab::new_with_source(
        CollabOrigin::Empty,
        &object_id,
        DataSource::DocStateV1(doc_state),
        vec![],
        false,
      )?;
      collab.initialize();
      let index_content = collab.subscribe_index_content();
      (MutexCollab::new(collab), index_content)
    };
    if !messages.is_empty() {
      Self::handle_collab_updates(&mut update_stream, &collab, messages).await?;
    }
    let mut tasks = JoinSet::new();
    tasks.spawn(Self::receive_collab_updates(
      update_stream,
      collab.downgrade(),
      ingest_interval,
    ));
    tasks.spawn(Self::receive_index_events(
      index_content,
      ai_client,
      object_id,
      workspace_id,
      collab_type,
      ingest_interval,
    ));

    Ok(Self { collab, tasks })
  }

  /// In regular time intervals, receive yrs updates and apply them to the locall in-memory collab
  /// representation. This should emit index content events, which we listen to on
  /// [Self::receive_index_events].
  async fn receive_collab_updates(
    mut update_stream: StreamGroup,
    collab: WeakMutexCollab,
    ingest_interval: Duration,
  ) {
    let mut interval = interval(ingest_interval);
    loop {
      interval.tick().await;
      let result = update_stream
        .consumer_messages(CONSUMER_NAME, ReadOption::Count(100))
        .await;
      match result {
        Ok(messages) if !messages.is_empty() => {
          if let Some(collab) = collab.upgrade() {
            match Self::handle_collab_updates(&mut update_stream, &collab, messages).await {
              Ok(_) => { /* no changes */ },
              Err(err) => tracing::error!("failed to handle messages: {}", err),
            }
          } else {
            tracing::trace!("collab dropped, stopping consumer");
            return;
          }
        },
        Ok(_) => { /* no messages */ },
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
  ) -> Result<()> {
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
    } else {
      tracing::error!("failed to lock collab");
    }
    update_stream.ack_messages(&messages).await?;

    Ok(())
  }

  /// Receive index content events and send them to the AI client for indexing.
  /// Debounce the events to avoid flooding the AI client with too many requests.
  async fn receive_index_events(
    mut stream: IndexContentReceiver,
    client: AppFlowyAIClient,
    object_id: String,
    workspace_id: String,
    collab_type: CollabType,
    interval: Duration,
  ) {
    let mut last_update_time = Instant::now();
    let mut last_update = None;
    loop {
      select! {
        _ = tokio::time::sleep(interval) => {
          if let Some(update) = last_update.take() {
            // debounce: if we're flooded with a lot of updates in short timespan,
            // we only emit the snapshot indexing request on the most recent one
            match Self::handle_index_event(&client, update, object_id.clone(), workspace_id.clone(), &collab_type).await {
              Ok(_) => {
                last_update_time = Instant::now();
                last_update = None;
              },
              Err(err) => tracing::error!("failed to handle index event: {}", err),
            }
          }
        },
        result = stream.recv() => {
          match result {
            Err(_) => {
              tracing::trace!("index content stream for {}/{} has been closed", workspace_id, object_id);
              return;
            }
            Ok(update) => {
              tracing::trace!("received index content event");
              let now = Instant::now();
              // check if enough time has passed since the last index update request
              if now - last_update_time >= interval {
                match Self::handle_index_event(&client, update, object_id.clone(), workspace_id.clone(), &collab_type).await {
                  Ok(_) => {
                    tracing::trace!("indexed document content");
                    last_update_time = now;
                    last_update = None;
                  },
                  Err(err) => tracing::error!("failed to handle index event: {}", err),
                }
              } else {
                // not enough time has passed - just store the update for the debouncer to hit
                last_update = Some(update);
              }
            }
          }
        },
      }
    }
  }

  /// Convert the collab type to the AI client's collab type.
  /// Return `None` if indexing of a documents of given type is not supported - it will not cause
  /// an error, but will be logged as a warning.
  fn map_collab_type(collab_entity: &CollabType) -> Option<appflowy_ai_client::dto::CollabType> {
    match collab_entity {
      CollabType::Document => Some(appflowy_ai_client::dto::CollabType::Document),
      CollabType::Database => Some(appflowy_ai_client::dto::CollabType::Database),
      CollabType::WorkspaceDatabase => Some(appflowy_ai_client::dto::CollabType::WorkspaceDatabase),
      CollabType::Folder => Some(appflowy_ai_client::dto::CollabType::Folder),
      CollabType::DatabaseRow => Some(appflowy_ai_client::dto::CollabType::DatabaseRow),
      CollabType::UserAwareness => None,
      CollabType::Unknown => None,
    }
  }

  #[instrument(skip(client, e))]
  async fn handle_index_event(
    client: &AppFlowyAIClient,
    e: IndexContent,
    object_id: String,
    workspace_id: String,
    collab_type: &CollabType,
  ) -> Result<()> {
    match e {
      IndexContent::Create(json) | IndexContent::Update(json) => {
        if let Some(collab_type) = Self::map_collab_type(collab_type) {
          let document = Document {
            id: object_id,
            doc_type: collab_type,
            workspace_id,
            content: json.to_string(),
          };
          client.index_documents(&[document]).await?;
        } else {
          tracing::warn!(
            "this collab type indexing is not supported: {:?}",
            collab_type
          );
        }
      },
      IndexContent::Delete(_json) => {
        //FIXME: implement unindexing of the documents
        tracing::warn!("unindexing of the documents is not implemented yet");
      },
    }
    Ok(())
  }
}

impl Drop for CollabHandle {
  fn drop(&mut self) {
    self.tasks.abort_all()
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

    let mut stream_group = redis_stream
      .collab_update_stream(&workspace_id, &object_id, "indexer")
      .await
      .unwrap();

    let _s = collab_update_forwarder(&mut collab, stream_group.clone());
    let doc = collab.get_doc();
    let init_state = doc.get_encoded_collab_v1().doc_state;
    let data_ref = doc.get_or_insert_map("data");
    collab.with_origin_transact_mut(|txn| {
      data_ref.insert(txn, "key", "value");
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

    println!("{:?}", docs);
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
