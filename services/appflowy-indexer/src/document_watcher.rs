use std::pin::Pin;
use std::sync::Arc;

use async_stream::stream;
use collab::core::collab::MutexCollab;
use collab_document::blocks::DeltaType;
use collab_document::document::Document;
use dashmap::DashMap;
use futures::Stream;
use tokio::sync::watch::Sender;
use yrs::types::ToJson;
use yrs::{GetString, Map, ReadTxn};

use crate::collab_handle::{DocumentUpdate, Indexable};
use crate::error::Result;

pub struct DocumentWatcher {
  object_id: String,
  workspace_id: String,
  content: Document,
  receiver: tokio::sync::watch::Receiver<DashMap<String, DeltaType>>,
}

unsafe impl Send for DocumentWatcher {}
unsafe impl Sync for DocumentWatcher {}

impl DocumentWatcher {
  pub fn new(object_id: String, workspace_id: String, mut content: Document) -> Result<Self> {
    let (tx, receiver) = tokio::sync::watch::channel(DashMap::new());
    Self::attach_listener(&mut content, tx);
    Ok(Self {
      object_id,
      workspace_id,
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

  pub fn as_stream(&self) -> Pin<Box<dyn Stream<Item = DocumentUpdate> + Send + Sync>> {
    let mut receiver = self.receiver.clone();
    let collab = Arc::downgrade(self.content.get_collab());
    let workspace_id = self.workspace_id.clone();
    let _object_id = self.object_id.clone();
    Box::pin(stream! {
      while let Ok(()) = receiver.changed().await {
        if let Some(collab) = collab.upgrade() {
          let updates = {
            let data = receiver.borrow();
            let lock = collab.lock();
            let tx = lock.transact();
            let text_map = {
              //TODO: make document navigator capable of reusing the transactions
              let root = tx.get_map("data").unwrap();
              let document = root.get(&tx, "document").unwrap().cast::<yrs::MapRef>().unwrap();
              let meta = document.get(&tx, "meta").unwrap().cast::<yrs::MapRef>().unwrap();
              meta.get(&tx, "text_map").unwrap().cast::<yrs::MapRef>().unwrap()
            };
            let updates: Vec<_> = data.iter().map(|entry| {
              let key = entry.key();
              let text = text_map.get(&tx, key).unwrap().cast::<yrs::TextRef>().unwrap();
              let block_content = text.get_string(&tx);
              match entry.value() {
                DeltaType::Inserted | DeltaType::Updated => {
                  DocumentUpdate::Update(appflowy_ai_client::dto::Document {
                    id: key.clone(),
                    doc_type: appflowy_ai_client::dto::CollabType::Document,
                    workspace_id: workspace_id.clone(),
                    content: block_content,
                  })
                }
                DeltaType::Removed => DocumentUpdate::Removed(key.clone()),
              }
            }).collect();
            data.clear();
            updates
          };
          for update in updates {
              yield update;
          }
        } else {
            break;
        }
      }
    })
  }
}

impl Indexable for DocumentWatcher {
  fn get_collab(&self) -> &MutexCollab {
    &*self.content.get_collab()
  }

  fn changes(&self) -> Pin<Box<dyn Stream<Item = DocumentUpdate> + Send + Sync>> {
    self.as_stream()
  }
}

#[cfg(test)]
mod test {
  use collab::core::collab::DataSource;
  use collab::core::origin::CollabOrigin;
  use collab_document::document::Document;
  use collab_document::document_data::default_document_collab_data;
  use serde_json::json;
  use tokio_stream::StreamExt;

  use crate::document_watcher::DocumentWatcher;

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

    let watcher = DocumentWatcher::new("o-1".to_string(), "w-1".to_string(), doc).unwrap();
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
      super::DocumentUpdate::Update(appflowy_ai_client::dto::Document {
        id: text_id.clone(),
        doc_type: appflowy_ai_client::dto::CollabType::Document,
        workspace_id: "w-1".to_string(),
        content: "A".to_string(),
      })
    );

    // modify text block again, we expect to have a cumulative content of the block
    watcher
      .content
      .apply_text_delta(&text_id, json!([{"insert": "B"}]).to_string());
    let update = stream.next().await.unwrap();

    assert_eq!(
      update,
      super::DocumentUpdate::Update(appflowy_ai_client::dto::Document {
        id: text_id,
        doc_type: appflowy_ai_client::dto::CollabType::Document,
        workspace_id: "w-1".to_string(),
        content: "BA".to_string(),
      })
    );
  }

  fn get_first_text_block(doc: &Document) -> collab_document::blocks::Block {
    let data = doc.get_document_data().unwrap();
    let blocks = &data.blocks;
    let (_, block) = blocks
      .iter()
        //TODO: Block::external_type should probably be an enum
      .filter(|(_, b)| b.external_type.as_deref() == Some("text"))
      .next()
      .unwrap();
    block.clone()
  }
}
