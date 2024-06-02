use std::pin::Pin;
use std::sync::Arc;

use async_stream::stream;
use collab::core::collab::MutexCollab;
use collab_document::blocks::{DeltaType, DocumentData, TextDelta};
use collab_document::document::Document;
use collab_entity::CollabType;
use dashmap::DashMap;
use database_entity::dto::EmbeddingContentType;
use futures::Stream;
use tokio::sync::watch::Sender;

use crate::collab_handle::{FragmentUpdate, Indexable};
use crate::error::Result;
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
    let content = Self::document_to_plain_text(&document_data);
    Ok(content)
  }

  fn document_to_plain_text(document: &DocumentData) -> String {
    let mut buf = String::new();
    if let Some(text_map) = document.meta.text_map.as_ref() {
      let mut stack = Vec::new();
      stack.push(&document.page_id);
      // do a depth-first scan of the document blocks
      while let Some(block_id) = stack.pop() {
        if let Some(block) = document.blocks.get(block_id) {
          if block.external_type.as_deref() == Some("text") {
            if let Some(text_id) = block.external_id.as_deref() {
              if let Some(json) = text_map.get(text_id) {
                match serde_json::from_str::<Vec<TextDelta>>(json) {
                  Ok(deltas) => {
                    for delta in deltas {
                      if let TextDelta::Inserted(text, _) = delta {
                        buf.push_str(&text);
                      }
                    }
                  },
                  Err(err) => {
                    tracing::error!("text_id `{}` is not a valid delta array: {}", text_id, err)
                  },
                }
              }
              buf.push('\n');
            }
          }
          if let Some(children) = document.meta.children_map.get(&block.children) {
            // we want to process children blocks in the same order they are given in children_map
            // however stack.pop gives us the last element first, so we push children
            // in reverse order
            stack.extend(children.iter().rev());
          }
        }
      }
    }
    buf
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
  use workspace_template::document::get_started::{
    get_initial_document_data, get_started_document_data,
  };

  use crate::watchers::DocumentWatcher;

  #[test]
  fn document_plain_text() {
    let doc = get_started_document_data().unwrap();
    let text = DocumentWatcher::document_to_plain_text(&doc);
    let expected = "\nWelcome to AppFlowy!\nHere are the basics\nClick anywhere and just start typing.\nHighlight any text, and use the editing menu to style your writing however you like.\nAs soon as you type / a menu will pop up. Select different types of content blocks you can add.\nType / followed by /bullet or /num to create a list.\nClick + New Page button at the bottom of your sidebar to add a new page.\nClick + next to any page title in the sidebar to quickly add a new subpage, Document, Grid, or Kanban Board.\n\n\nKeyboard shortcuts, markdown, and code block\nKeyboard shortcuts guide\nMarkdown reference\nType /code to insert a code block\n// This is the main function.\nfn main() {\n    // Print text to the console.\n    println!(\"Hello World!\");\n}\n\nHave a question❓\nClick ? at the bottom right for help and support.\n\n\nLike AppFlowy? Follow us:\nGitHub\nTwitter: @appflowy\nNewsletter\n\n\n\n\n";
    assert_eq!(&text, expected);
  }

  #[test]
  fn document_plain_text_with_nested_blocks() {
    let doc = get_initial_document_data().unwrap();
    let text = DocumentWatcher::document_to_plain_text(&doc);
    let expected = "Welcome to AppFlowy!\nHere are the basics\nHere is H3\nClick anywhere and just start typing.\nClick Enter to create a new line.\nHighlight any text, and use the editing menu to style your writing however you like.\nAs soon as you type / a menu will pop up. Select different types of content blocks you can add.\nType / followed by /bullet or /num to create a list.\nClick + New Page button at the bottom of your sidebar to add a new page.\nClick + next to any page title in the sidebar to quickly add a new subpage, Document, Grid, or Kanban Board.\n\n\nKeyboard shortcuts, markdown, and code block\nKeyboard shortcuts guide\nMarkdown reference\nType /code to insert a code block\n// This is the main function.\nfn main() {\n    // Print text to the console.\n    println!(\"Hello World!\");\n}\n\nThis is a paragraph\nThis is a paragraph\nHave a question❓\nClick ? at the bottom right for help and support.\nThis is a paragraph\nThis is a paragraph\nClick ? at the bottom right for help and support.\n\n\nLike AppFlowy? Follow us:\nGitHub\nTwitter: @appflowy\nNewsletter\n\n\n\n\n";
    assert_eq!(&text, expected);
  }

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
