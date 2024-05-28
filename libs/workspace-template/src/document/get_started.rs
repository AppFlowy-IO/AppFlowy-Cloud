use crate::document::parser::JsonToDocumentParser;
use crate::hierarchy_builder::WorkspaceViewBuilder;
use crate::{TemplateData, WorkspaceTemplate};
use anyhow::Error;
use async_trait::async_trait;
use collab::core::collab::MutexCollab;
use collab::core::origin::CollabOrigin;
use collab::preclude::Collab;
use collab_document::blocks::DocumentData;
use collab_document::document::Document;
use collab_entity::CollabType;
use collab_folder::ViewLayout;
use std::sync::Arc;
use tokio::sync::RwLock;

/// This template generates a document containing a 'read me' guide.
/// It ensures that at least one view is created for the document.
pub struct GetStartedDocumentTemplate;

#[async_trait]
impl WorkspaceTemplate for GetStartedDocumentTemplate {
  fn layout(&self) -> ViewLayout {
    ViewLayout::Document
  }

  async fn create(&self, object_id: String) -> anyhow::Result<TemplateData> {
    let data = tokio::task::spawn_blocking(|| {
      let document_data = get_started_document_data().unwrap();
      let collab = Arc::new(MutexCollab::new(Collab::new_with_origin(
        CollabOrigin::Empty,
        &object_id,
        vec![],
        false,
      )));
      let document = Document::create_with_data(collab, document_data)?;
      let data = document.encode_collab()?;
      Ok::<_, anyhow::Error>(TemplateData {
        object_id,
        object_type: CollabType::Document,
        object_data: data,
      })
    })
    .await??;
    Ok(data)
  }

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

    self.create(view_id).await
  }
}

pub enum DocumentTemplateContent {
  Json(String),
  Data(DocumentData),
}

/// Create a document with the given content
pub struct DocumentTemplate(DocumentData);

impl DocumentTemplate {
  pub fn from_json(json: &str) -> Result<Self, Error> {
    let data = JsonToDocumentParser::json_str_to_document(json)?;
    Ok(Self(data))
  }

  pub fn from_data(data: DocumentData) -> Self {
    Self(data)
  }
}

#[async_trait]
impl WorkspaceTemplate for DocumentTemplate {
  fn layout(&self) -> ViewLayout {
    ViewLayout::Document
  }

  async fn create(&self, object_id: String) -> anyhow::Result<TemplateData> {
    let collab = Arc::new(MutexCollab::new(Collab::new_with_origin(
      CollabOrigin::Empty,
      &object_id,
      vec![],
      false,
    )));
    let document = Document::create_with_data(collab, self.0.clone())?;
    let data = document.encode_collab()?;
    Ok(TemplateData {
      object_id,
      object_type: CollabType::Document,
      object_data: data,
    })
  }

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
    self.create(view_id).await
  }
}

pub fn get_started_document_data() -> Result<DocumentData, Error> {
  let json_str = include_str!("../../assets/read_me.json");
  JsonToDocumentParser::json_str_to_document(json_str)
}

pub fn get_initial_document_data() -> Result<DocumentData, Error> {
  let json_str = include_str!("../../assets/initial_document.json");
  JsonToDocumentParser::json_str_to_document(json_str)
}
