use std::collections::HashMap;

use std::sync::Arc;

use anyhow::Error;
use async_trait::async_trait;
use collab::core::origin::CollabOrigin;
use collab::preclude::Collab;
use collab_database::database::DatabaseData;
use collab_database::entity::CreateDatabaseParams;
use collab_document::blocks::DocumentData;
use collab_document::document::Document;
use collab_entity::CollabType;
use collab_folder::ViewLayout;
use serde_json::Value;
use tokio::sync::RwLock;

use crate::database::database::{create_database_collab, create_database_row_collabs};
use crate::document::parser::JsonToDocumentParser;
use crate::hierarchy_builder::WorkspaceViewBuilder;
use crate::{gen_view_id, TemplateData, WorkspaceTemplate};

// Template Folder Structure:
// |-- Getting started (document)
// |   |-- Desktop guide (document)
// |   |-- Mobile guide (document)
// |-- To-Dos (board)
// |   |-- Inbox (grid)
// |-- Ask AI (ai chat)
//
// Note: Update the folder structure above if you changed the code below
pub struct GettingStartedTemplate;

impl GettingStartedTemplate {
  /// Create a document template data from the given JSON string
  async fn create_document_from_json(
    &self,
    object_id: String,
    json_str: &str,
  ) -> anyhow::Result<TemplateData> {
    // 1. read the getting started document from the assets
    let document_data = JsonToDocumentParser::json_str_to_document(json_str)?;

    // 2. create a new document with the getting started data
    let data = tokio::task::spawn_blocking(move || {
      let collab = Collab::new_with_origin(CollabOrigin::Empty, &object_id, vec![], false);
      let document = Document::open_with(collab, Some(document_data))?;
      let data = document.encode_collab()?;

      Ok::<_, anyhow::Error>(TemplateData {
        object_id,
        object_type: CollabType::Document,
        object_data: data,
        database_id: None,
      })
    })
    .await??;

    Ok(data)
  }

  /// Create a series of database templates from the given JSON String
  ///
  /// Notes: The output contains DatabaseCollab, DatabaseRowCollab
  async fn create_database_from_params(
    &self,
    object_id: String,
    create_database_params: CreateDatabaseParams,
  ) -> anyhow::Result<Vec<TemplateData>> {
    let database_id = create_database_params.database_id.clone();

    let data = tokio::task::spawn_blocking(move || {
      // 2. create a new database collab  with the create database params
      let collab = create_database_collab(object_id.clone(), create_database_params.clone())?;
      let data =
        collab.encode_collab_v1(|collab| CollabType::Database.validate_require_data(collab))?;
      let database_template_data = TemplateData {
        object_id: object_id.clone(),
        object_type: CollabType::Database,
        object_data: data,
        database_id: Some(database_id),
      };

      // 3. create the new database row collabs
      let row_collabs = create_database_row_collabs(object_id.clone(), create_database_params)?;
      let row_template_data = row_collabs.iter().map(|(id, collab)| {
        let data = collab
          .encode_collab_v1(|collab| CollabType::DatabaseRow.validate_require_data(collab))
          .unwrap();
        TemplateData {
          object_id: id.clone(),
          object_type: CollabType::DatabaseRow,
          object_data: data,
          database_id: None,
        }
      });

      let mut template_data = vec![database_template_data];
      template_data.extend(row_template_data);

      Ok::<_, anyhow::Error>(template_data)
    })
    .await??;
    Ok(data)
  }
}

#[async_trait]
impl WorkspaceTemplate for GettingStartedTemplate {
  fn layout(&self) -> ViewLayout {
    ViewLayout::Document
  }

  async fn create(&self, _object_id: String) -> anyhow::Result<Vec<TemplateData>> {
    unreachable!("This function is not supposed to be called.")
  }

  async fn create_workspace_view(
    &self,
    _uid: i64,
    workspace_view_builder: Arc<RwLock<WorkspaceViewBuilder>>,
  ) -> anyhow::Result<Vec<TemplateData>> {
    let getting_started_view_uuid = gen_view_id().to_string();
    let desktop_guide_view_uuid = gen_view_id().to_string();
    let mobile_guide_view_uuid = gen_view_id().to_string();
    let todos_view_uuid = gen_view_id().to_string();

    let getting_started_json = include_str!("../../assets/getting_started.json");
    let mut getting_started_json: Value = serde_json::from_str(getting_started_json).unwrap();
    let mut replacements = HashMap::new();
    replacements.insert(
      "desktop_guide_id".to_string(),
      desktop_guide_view_uuid.clone(),
    );
    replacements.insert(
      "mobile_guide_id".to_string(),
      mobile_guide_view_uuid.clone(),
    );
    replacements.insert("todos_id".to_string(), todos_view_uuid.clone());
    replace_json_placeholders(&mut getting_started_json, &replacements);
    let getting_started_data = self
      .create_document_from_json(
        getting_started_view_uuid.clone(),
        &getting_started_json.to_string(),
      )
      .await?;

    let desktop_guide_json = include_str!("../../assets/desktop_guide.json");
    let desktop_guide_data = self
      .create_document_from_json(desktop_guide_view_uuid.clone(), desktop_guide_json)
      .await?;

    let mobile_guide_json = include_str!("../../assets/mobile_guide.json");
    let mobile_guide_data = self
      .create_document_from_json(mobile_guide_view_uuid.clone(), mobile_guide_json)
      .await?;

    let todos_json = include_str!("../../assets/to-dos.json");
    let database_data = serde_json::from_str::<DatabaseData>(todos_json)?;
    let create_database_params = CreateDatabaseParams::from_database_data(database_data);
    let todos_data = self
      .create_database_from_params(todos_view_uuid.clone(), create_database_params.clone())
      .await?;

    let mut builder = workspace_view_builder.write().await;

    builder
      .with_view_builder(|view_builder| async {
        let getting_started_view_uuid = getting_started_view_uuid.clone();
        let desktop_guide_view_uuid = desktop_guide_view_uuid.clone();
        let mobile_guide_view_uuid = mobile_guide_view_uuid.clone();

        // getting started view
        let mut view_builder = view_builder
          .with_name("Getting started")
          .with_icon("⭐️")
          .with_view_id(getting_started_view_uuid);

        view_builder = view_builder
          .with_child_view_builder(|child_view_builder| async {
            // desktop guide view
            let desktop_guide_view_uuid = desktop_guide_view_uuid.clone();
            child_view_builder
              .with_name("Desktop guide")
              .with_icon("🖥️")
              .with_view_id(desktop_guide_view_uuid)
              .build()
          })
          .await;

        view_builder = view_builder
          .with_child_view_builder(|child_view_builder| async {
            // mobile guide view
            let mobile_guide_view_uuid = mobile_guide_view_uuid.clone();
            child_view_builder
              .with_name("Mobile guide")
              .with_icon("📱")
              .with_view_id(mobile_guide_view_uuid)
              .build()
          })
          .await;

        view_builder.build()
      })
      .await;

    builder
      .with_view_builder(|view_builder| async {
        let view_builder = view_builder
          .with_layout(ViewLayout::Board)
          .with_view_id(todos_view_uuid.clone())
          .with_name("To-Dos")
          .with_icon("✅");

        view_builder.build()
      })
      .await;

    let mut template_data = vec![getting_started_data, desktop_guide_data, mobile_guide_data];
    template_data.extend(todos_data);
    Ok(template_data)
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

  async fn create(&self, object_id: String) -> anyhow::Result<Vec<TemplateData>> {
    let collab = Collab::new_with_origin(CollabOrigin::Empty, &object_id, vec![], false);
    let document = Document::open_with(collab, Some(self.0.clone()))?;
    let data = document.encode_collab()?;
    Ok(vec![TemplateData {
      object_id,
      object_type: CollabType::Document,
      object_data: data,
      database_id: None,
    }])
  }

  async fn create_workspace_view(
    &self,
    _uid: i64,
    workspace_view_builder: Arc<RwLock<WorkspaceViewBuilder>>,
  ) -> anyhow::Result<Vec<TemplateData>> {
    let view_id = gen_view_id().to_string();

    let mut builder = workspace_view_builder.write().await;
    builder
      .with_view_builder(|view_builder| async {
        view_builder
          .with_name("Getting started")
          .with_icon("⭐️")
          .with_view_id(view_id.clone())
          .build()
      })
      .await;

    self.create(view_id).await
  }
}

pub fn getting_started_document_data() -> Result<DocumentData, Error> {
  let json_str = include_str!("../../assets/getting_started.json");
  JsonToDocumentParser::json_str_to_document(json_str)
}

pub fn desktop_guide_document_data() -> Result<DocumentData, Error> {
  let json_str = include_str!("../../assets/desktop_guide.json");
  JsonToDocumentParser::json_str_to_document(json_str)
}

pub fn mobile_guide_document_data() -> Result<DocumentData, Error> {
  let json_str = include_str!("../../assets/mobile_guide.json");
  JsonToDocumentParser::json_str_to_document(json_str)
}

pub fn get_initial_document_data() -> Result<DocumentData, Error> {
  let json_str = include_str!("../../assets/initial_document.json");
  JsonToDocumentParser::json_str_to_document(json_str)
}

/// Replace the placeholders in the JSON value with the given replacements.
///
/// The placeholders are in the format of "<key>", for example "<name>".
/// The value of the placeholder will be replaced with the value of the key in the replacements map.
fn replace_json_placeholders(value: &mut Value, replacements: &HashMap<String, String>) {
  match value {
    Value::String(s) => {
      if s.starts_with("<") && s.ends_with(">") {
        let key = s.trim_start_matches("<").trim_end_matches(">");
        if let Some(replacement) = replacements.get(key) {
          *s = replacement.to_string();
        }
      }
    },
    Value::Array(arr) => {
      for item in arr {
        replace_json_placeholders(item, replacements);
      }
    },
    Value::Object(obj) => {
      for (_, v) in obj {
        replace_json_placeholders(v, replacements);
      }
    },
    _ => {},
  }
}
