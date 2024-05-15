use appflowy_ai_client::dto::CollabType;
use collab::core::collab::{DataSource, MutexCollab};
use collab::core::origin::CollabOrigin;
use collab_document::blocks::DeltaType;
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
            DeltaType::Removed => DocumentUpdate::Removed(payload.id.clone()),
            DeltaType::Inserted | DeltaType::Updated => {
              let doc = appflowy_ai_client::dto::Document {
                id: payload.id.clone(),
                doc_type: CollabType::Document,
                workspace_id: workspace_id.clone(),
                content: payload.value.clone(),
              };
              DocumentUpdate::Update(doc)
            },
          };
          let _ = channel.send(update);
        }
      }
    })
  }
}
