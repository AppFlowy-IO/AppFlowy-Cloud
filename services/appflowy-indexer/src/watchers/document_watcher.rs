use std::pin::Pin;
use std::sync::Arc;

use async_stream::stream;
use collab::core::collab::MutexCollab;
use collab_document::document::Document;
use collab_entity::CollabType;
use database_entity::dto::EmbeddingContentType;
use futures::Stream;
use tokio::sync::watch::Sender;
use yrs::Subscription;

use crate::collab_handle::{FragmentUpdate, Indexable};
use crate::error::Result;
use crate::extract::document_to_plain_text;
use crate::indexer::Fragment;

pub struct DocumentWatcher {
  object_id: String,
  content: Arc<MutexCollab>,
  receiver: tokio::sync::watch::Receiver<u64>,
  #[allow(dead_code)]
  update_observer: Subscription,
}

unsafe impl Send for DocumentWatcher {}
unsafe impl Sync for DocumentWatcher {}

impl DocumentWatcher {
  pub fn new(
    object_id: String,
    content: Arc<MutexCollab>,
    index_initial_content: bool,
  ) -> Result<Self> {
    let (tx, receiver) = tokio::sync::watch::channel(0);
    if index_initial_content {
      Self::index_initial_content(content.clone(), &tx)?;
    }
    let update_observer = Self::attach_listener(&content, tx);
    Ok(Self {
      object_id,
      content,
      receiver,
      update_observer,
    })
  }

  fn attach_listener(document: &Arc<MutexCollab>, notifier: Sender<u64>) -> Subscription {
    let lock = document.lock();
    lock
      .get_doc()
      .observe_update_v1(move |_, _| {
        notifier.send_modify(|i| *i += 1);
      })
      .unwrap()
  }

  fn index_initial_content(collab: Arc<MutexCollab>, notifier: &Sender<u64>) -> Result<()> {
    match Document::open(collab.clone()) {
      Ok(document) => match document.get_document_data() {
        Ok(data) => {
          if data.meta.text_map.as_ref().is_some() {
            notifier.send_modify(|i| *i += 1);
          }
        },
        Err(err) => {
          tracing::warn!("Failed to get document data: {}", err);
        },
      },
      Err(err) => {
        tracing::warn!("Failed to open document: {}", err);
      },
    }
    Ok(())
  }

  pub fn as_stream(&self) -> Pin<Box<dyn Stream<Item = FragmentUpdate> + Send + Sync>> {
    let mut receiver = self.receiver.clone();
    let collab = Arc::downgrade(&self.content);
    let object_id = self.object_id.clone();
    Box::pin(stream! {
      while let Ok(()) = receiver.changed().await {
        if let Some(collab) = collab.upgrade() {
          match Self::get_document_content(collab) {
            Ok(content) => {
              yield FragmentUpdate::Update(Fragment {
                fragment_id: object_id.clone(),
                collab_type: CollabType::Document,
                content_type: EmbeddingContentType::PlainText,
                object_id: object_id.clone(),
                content,
              });
            },
            Err(err) => tracing::error!("Failed to get document content: {}", err),
          }
        } else {
            break;
        }
      }
    })
  }

  fn get_document_content(collab: Arc<MutexCollab>) -> Result<String> {
    let document = Document::open(collab)?;
    let document_data = document.get_document_data()?;
    let content = document_to_plain_text(&document_data);
    Ok(content)
  }
}

impl Indexable for DocumentWatcher {
  fn get_collab(&self) -> &MutexCollab {
    &self.content
  }

  fn changes(&self) -> Pin<Box<dyn Stream<Item = FragmentUpdate> + Send + Sync>> {
    self.as_stream()
  }
}

#[cfg(test)]
mod test {
  use crate::indexer::Fragment;
  use collab::core::collab::{DataSource, MutexCollab};
  use collab::core::origin::CollabOrigin;
  use collab::preclude::Collab;
  use collab_document::document::Document;
  use collab_document::document_data::default_document_collab_data;
  use collab_entity::CollabType;
  use database_entity::dto::EmbeddingContentType;
  use serde_json::json;
  use std::sync::Arc;
  use tokio_stream::StreamExt;

  use crate::watchers::DocumentWatcher;

  #[tokio::test]
  async fn test_update_generation() {
    let _ = env_logger::builder().is_test(true).try_init();

    let encoded_collab = default_document_collab_data("o-1").unwrap();
    let collab = Arc::new(MutexCollab::new(
      Collab::new_with_source(
        CollabOrigin::Empty,
        "o-1",
        DataSource::DocStateV1(encoded_collab.doc_state.into()),
        vec![],
        false,
      )
      .unwrap(),
    ));
    let doc = Document::open(collab.clone()).unwrap();

    let watcher = DocumentWatcher::new("o-1".to_string(), collab, true).unwrap();
    let mut stream = watcher.as_stream();

    let text_id = {
      // modify the text block for the first time
      let block = get_first_text_block(&doc);
      let text_id = block.external_id.clone().unwrap();
      doc.apply_text_delta(&text_id, json!([{"insert": "A"}]).to_string());
      text_id
    };

    let update = stream.next().await.unwrap();
    assert_eq!(
      update,
      super::FragmentUpdate::Update(Fragment {
        fragment_id: "o-1".to_string(),
        collab_type: CollabType::Document,
        content_type: EmbeddingContentType::PlainText,
        object_id: "o-1".to_string(),
        content: "A ".to_string(),
      })
    );

    // modify text block again, we expect to have a cumulative content of the block
    doc.apply_text_delta(&text_id, json!([{"insert": "B"}]).to_string());
    let update = stream.next().await.unwrap();

    assert_eq!(
      update,
      super::FragmentUpdate::Update(Fragment {
        fragment_id: "o-1".to_string(),
        collab_type: CollabType::Document,
        content_type: EmbeddingContentType::PlainText,
        object_id: "o-1".to_string(),
        content: "BA ".to_string(),
      })
    );
  }

  fn get_first_text_block(doc: &Document) -> collab_document::blocks::Block {
    let data = doc.get_document_data().unwrap();
    let blocks = &data.blocks;
    let (_, block) = blocks
      .iter()
      .find(|(_, b)| b.external_type.as_deref() == Some("text"))
      .unwrap();
    block.clone()
  }
}
