use std::pin::Pin;
use std::sync::Arc;

use async_stream::stream;
use collab::core::collab::MutexCollab;
use collab_document::blocks::DeltaType;
use collab_document::document::Document;
use collab_entity::CollabType;
use dashmap::DashMap;
use database_entity::dto::EmbeddingContentType;
use futures::Stream;
use tokio::sync::watch::Sender;

use crate::collab_handle::{FragmentUpdate, Indexable};
use crate::error::Result;
use crate::extract::document_to_plain_text;
use crate::indexer::Fragment;

pub struct DocumentWatcher {
  object_id: String,
  content: Document,
  receiver: tokio::sync::watch::Receiver<DashMap<String, DeltaType>>,
}

unsafe impl Send for DocumentWatcher {}
unsafe impl Sync for DocumentWatcher {}

impl DocumentWatcher {
  pub fn new(
    object_id: String,
    mut content: Document,
    index_initial_content: bool,
  ) -> Result<Self> {
    let (tx, receiver) = tokio::sync::watch::channel(DashMap::new());
    if index_initial_content {
      Self::index_initial_content(&mut content, &tx)?;
    }
    Self::attach_listener(&mut content, tx);
    Ok(Self {
      object_id,
      content,
      receiver,
    })
  }

  fn attach_listener(document: &mut Document, notifier: Sender<DashMap<String, DeltaType>>) {
    document.subscribe_block_changed(move |blocks, _| {
      let changes: Vec<_> = blocks
        .iter()
        .flat_map(|block| {
          block
            .iter()
            .map(|payload| (payload.id.clone(), payload.command.clone()))
        })
        .collect();
      notifier.send_modify(|map| {
        for (id, command) in changes {
          map.insert(id, command);
        }
      })
    });
  }

  fn index_initial_content(
    document: &mut Document,
    notifier: &Sender<DashMap<String, DeltaType>>,
  ) -> Result<()> {
    let data = document.get_document_data()?;
    if let Some(text_map) = data.meta.text_map.as_ref() {
      notifier.send_modify(|map| {
        for text_id in text_map.keys() {
          map.insert(text_id.clone(), DeltaType::Inserted);
        }
      });
    }
    Ok(())
  }

  pub fn as_stream(&self) -> Pin<Box<dyn Stream<Item = FragmentUpdate> + Send + Sync>> {
    let mut receiver = self.receiver.clone();
    let collab = Arc::downgrade(self.content.get_collab());
    let object_id = self.object_id.clone();
    Box::pin(stream! {
      while let Ok(()) = receiver.changed().await {
        if let Some(collab) = collab.upgrade() {
          receiver.borrow().clear();
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
    self.content.get_collab()
  }

  fn changes(&self) -> Pin<Box<dyn Stream<Item = FragmentUpdate> + Send + Sync>> {
    self.as_stream()
  }
}

#[cfg(test)]
mod test {
  use crate::indexer::Fragment;
  use collab::core::collab::DataSource;
  use collab::core::origin::CollabOrigin;
  use collab_document::document::Document;
  use collab_document::document_data::default_document_collab_data;
  use collab_entity::CollabType;
  use database_entity::dto::EmbeddingContentType;
  use serde_json::json;
  use tokio_stream::StreamExt;

  use crate::watchers::DocumentWatcher;

  #[tokio::test]
  async fn test_update_generation() {
    let _ = env_logger::builder().is_test(true).try_init();

    let encoded_collab = default_document_collab_data("o-1").unwrap();
    let doc = Document::from_doc_state(
      CollabOrigin::Empty,
      DataSource::DocStateV1(encoded_collab.doc_state.into()),
      "o-1",
      vec![],
    )
    .unwrap();

    let watcher = DocumentWatcher::new("o-1".to_string(), doc, true).unwrap();
    let mut stream = watcher.as_stream();

    let text_id = {
      // modify the text block for the first time
      let block = get_first_text_block(&watcher.content);
      let text_id = block.external_id.clone().unwrap();
      watcher
        .content
        .apply_text_delta(&text_id, json!([{"insert": "A"}]).to_string());
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
        content: "A\n".to_string(),
      })
    );

    // modify text block again, we expect to have a cumulative content of the block
    watcher
      .content
      .apply_text_delta(&text_id, json!([{"insert": "B"}]).to_string());
    let update = stream.next().await.unwrap();

    assert_eq!(
      update,
      super::FragmentUpdate::Update(Fragment {
        fragment_id: "o-1".to_string(),
        collab_type: CollabType::Document,
        content_type: EmbeddingContentType::PlainText,
        object_id: "o-1".to_string(),
        content: "BA\n".to_string(),
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
