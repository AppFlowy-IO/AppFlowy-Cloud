mod parser;

use crate::document::parser::JsonToDocumentParser;
use crate::hierarchy_builder::WorkspaceViewBuilder;
use crate::{TemplateData, WorkspaceTemplate};
use async_trait::async_trait;
use collab::core::collab::MutexCollab;
use collab::core::origin::CollabOrigin;
use collab_document::document::Document;
use collab_entity::CollabType;
use std::sync::Arc;
use tokio::sync::RwLock;

/// A default template for creating documents.
///
/// This template generates a document containing a 'read me' guide.
/// It ensures that at least one view is created for the document.
pub struct DocumentTemplate;

#[async_trait]
impl WorkspaceTemplate for DocumentTemplate {
  async fn create_workspace_view(
    &self,
    _uid: i64,
    workspace_view_builder: Arc<RwLock<WorkspaceViewBuilder>>,
  ) -> anyhow::Result<TemplateData> {
    let view_id = workspace_view_builder
      .write()
      .await
      .with_view_builder(|view_builder| async {
        view_builder
          .with_name("Getting started")
          .with_icon("⭐️")
          .build()
      })
      .await;

    // create a empty document
    let data = tokio::task::spawn_blocking(|| {
      let json_str = include_str!("../../assets/read_me.json");
      let document_data = JsonToDocumentParser::json_str_to_document(json_str).unwrap();
      let collab = Arc::new(MutexCollab::new(CollabOrigin::Empty, &view_id, vec![]));
      let document = Document::create_with_data(collab, document_data)?;
      let data = document.get_collab().encode_collab_v1();
      Ok::<_, anyhow::Error>(TemplateData {
        object_id: view_id,
        object_type: CollabType::Document,
        object_data: data,
      })
    })
    .await??;
    Ok(data)
  }
}
