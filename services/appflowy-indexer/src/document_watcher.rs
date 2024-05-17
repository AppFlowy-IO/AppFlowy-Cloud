use appflowy_ai_client::dto::CollabType;
use collab::core::collab::{DataSource, MutexCollab};
use collab::core::origin::CollabOrigin;
use collab_document::blocks::{DeltaType, TextDelta};
use collab_document::document::Document;
use tokio::sync::mpsc::UnboundedSender;

use crate::collab_handle::{DocumentUpdate, Indexable};
use crate::error::Result;

pub struct DocumentWatcher {
  object_id: String,
  workspace_id: String,
  content: Document,
}

unsafe impl Send for DocumentWatcher {}
unsafe impl Sync for DocumentWatcher {}

impl DocumentWatcher {
  pub fn new(object_id: String, workspace_id: String, state: Vec<u8>) -> Result<Self> {
    let content = Document::from_doc_state(
      CollabOrigin::Empty,
      DataSource::DocStateV1(state),
      &object_id,
      vec![],
    )?;
    Ok(Self {
      object_id,
      workspace_id,
      content,
    })
  }

  fn parse_block_value(json: &str) -> Option<String> {
    let delta: Vec<TextDelta> = serde_json::from_str(json).ok()?;
    let mut buf = String::new();
    for d in delta {
      match d {
        TextDelta::Inserted(text, _) => buf.push_str(&text),
        TextDelta::Deleted(_) => unreachable!("Should not have deleted deltas"),
        TextDelta::Retain(_, _) => unreachable!("Should not have retain deltas"),
      }
    }
    if buf.is_empty() {
      None
    } else {
      Some(buf)
    }
  }
}

impl Indexable for DocumentWatcher {
  fn get_collab(&self) -> &MutexCollab {
    &*self.content.get_collab()
  }

  fn attach_listener(&mut self, channel: UnboundedSender<DocumentUpdate>) {
    let workspace_id = self.workspace_id.clone();
    self.content.subscribe_block_changed(move |events, _| {
      for event in events {
        for payload in event.iter() {
          let update = match payload.command {
            DeltaType::Removed => Some(DocumentUpdate::Removed(payload.id.clone())),
            DeltaType::Inserted | DeltaType::Updated => {
              if let Some(doc) = Self::parse_block_value(&payload.value) {
                let doc = appflowy_ai_client::dto::Document {
                  id: payload.id.clone(),
                  doc_type: CollabType::Document,
                  workspace_id: workspace_id.clone(),
                  content: doc,
                };
                Some(DocumentUpdate::Update(doc))
              } else {
                None
              }
            },
          };
          if let Some(update) = update {
            let _ = channel.send(update);
          }
        }
      }
    })
  }
}
