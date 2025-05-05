use std::collections::HashMap;

use anyhow::Error;
use async_trait::async_trait;
use collab::core::origin::CollabOrigin;

use collab::preclude::Collab;
use collab_database::database::{timestamp, DatabaseData};
use collab_database::entity::CreateDatabaseParams;
use collab_document::blocks::DocumentData;
use collab_document::document::Document;
use collab_entity::CollabType;
use collab_folder::ViewLayout;
use serde_json::Value;

use crate::document::parser::JsonToDocumentParser;
use crate::document::util::{create_database_from_params, create_document_from_json};
use crate::hierarchy_builder::{ViewBuilder, WorkspaceViewBuilder};
use crate::{gen_view_id, TemplateData, TemplateObjectId, WorkspaceTemplate};

// Template Folder Structure:
// |-- General (space)
//     |-- Getting started (document)
//          |-- Desktop guide (document)
//          |-- Mobile guide (document)
//          |-- Web guide (document)
//     |-- To-dos (board)
// |-- Shared (space)
//     |-- ... (empty)
// Note: Update the folder structure above if you changed the code below
pub struct GettingStartedTemplate;

impl GettingStartedTemplate {
  /// Create a document template data from the given JSON string
  ///
  /// Create a series of database templates from the given JSON String
  ///
  /// Notes: The output contains DatabaseCollab, DatabaseRowCollab
  #[allow(clippy::too_many_arguments)]
  async fn create_document_and_database_data(
    &self,
    general_view_uuid: String,
    shared_view_uuid: String,
    getting_started_view_uuid: String,
    desktop_guide_view_uuid: String,
    mobile_guide_view_uuid: String,
    web_guide_view_uuid: String,
    todos_view_uuid: String,
  ) -> anyhow::Result<(
    TemplateData,
    TemplateData,
    TemplateData,
    TemplateData,
    TemplateData,
    TemplateData,
    Vec<TemplateData>,
  )> {
    let default_space_json = include_str!("../../assets/default_space.json");
    let general_data =
      create_document_from_json(general_view_uuid.clone(), default_space_json).await?;

    let shared_data =
      create_document_from_json(shared_view_uuid.clone(), default_space_json).await?;

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
    replacements.insert("web_guide_id".to_string(), web_guide_view_uuid.clone());
    replacements.insert("todos_id".to_string(), todos_view_uuid.clone());
    replace_json_placeholders(&mut getting_started_json, &replacements);
    let getting_started_data = create_document_from_json(
      getting_started_view_uuid.clone(),
      &getting_started_json.to_string(),
    )
    .await?;

    let desktop_guide_json = include_str!("../../assets/desktop_guide.json");
    let desktop_guide_data =
      create_document_from_json(desktop_guide_view_uuid.clone(), desktop_guide_json).await?;

    let mobile_guide_json = include_str!("../../assets/mobile_guide.json");
    let mobile_guide_data =
      create_document_from_json(mobile_guide_view_uuid.clone(), mobile_guide_json).await?;

    let web_guide_json = include_str!("../../assets/web_guide.json");
    let web_guide_data =
      create_document_from_json(web_guide_view_uuid.clone(), web_guide_json).await?;

    let todos_json = include_str!("../../assets/to-dos.json");
    let database_data = serde_json::from_str::<DatabaseData>(todos_json)?;
    let database_view_id = database_data.views[0].id.clone();
    let create_database_params =
      CreateDatabaseParams::from_database_data(database_data, &database_view_id, &todos_view_uuid);
    let todos_data =
      create_database_from_params(todos_view_uuid.clone(), create_database_params.clone()).await?;

    Ok((
      general_data,
      shared_data,
      getting_started_data,
      desktop_guide_data,
      mobile_guide_data,
      web_guide_data,
      todos_data,
    ))
  }

  async fn build_getting_started_view(
    &self,
    view_builder: ViewBuilder,
    getting_started_view_uuid: String,
    desktop_guide_view_uuid: String,
    mobile_guide_view_uuid: String,
    web_guide_view_uuid: String,
  ) -> ViewBuilder {
    // getting started view
    let mut view_builder = view_builder
      .with_name("Getting started")
      .with_icon("🌟")
      .with_extra(r#"{"font_layout":"normal","line_height_layout":"normal","cover":{"type":"gradient","value":"appflowy_them_color_gradient4"},"font":null}"#)
      .with_view_id(getting_started_view_uuid);

    view_builder = view_builder
      .with_child_view_builder({
        |child_view_builder| async {
          // desktop guide view
          let desktop_guide_view_uuid = desktop_guide_view_uuid.clone();
          child_view_builder
            .with_name("Desktop guide")
            .with_icon("📎")
            .with_view_id(desktop_guide_view_uuid)
            .build()
        }
      })
      .await;

    view_builder = view_builder
      .with_child_view_builder({
        |child_view_builder| async {
          // mobile guide view
          let mobile_guide_view_uuid = mobile_guide_view_uuid.clone();
          child_view_builder
            .with_name("Mobile guide")
            .with_view_id(mobile_guide_view_uuid)
            .build()
        }
      })
      .await;

    view_builder = view_builder
      .with_child_view_builder({
        |child_view_builder| async {
          // web guide view
          let web_guide_view_uuid = web_guide_view_uuid.clone();
          child_view_builder
            .with_name("Web guide")
            .with_view_id(web_guide_view_uuid)
            .build()
        }
      })
      .await;

    view_builder
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
    workspace_view_builder: &mut WorkspaceViewBuilder,
  ) -> anyhow::Result<Vec<TemplateData>> {
    let general_view_uuid = gen_view_id().to_string();
    let shared_view_uuid = gen_view_id().to_string();
    let getting_started_view_uuid = gen_view_id().to_string();
    let desktop_guide_view_uuid = gen_view_id().to_string();
    let mobile_guide_view_uuid = gen_view_id().to_string();
    let web_guide_view_uuid = gen_view_id().to_string();
    let todos_view_uuid = gen_view_id().to_string();

    let (
      general_data,
      shared_data,
      getting_started_data,
      desktop_guide_data,
      mobile_guide_data,
      web_guide_data,
      todos_data,
    ) = self
      .create_document_and_database_data(
        general_view_uuid.clone(),
        shared_view_uuid.clone(),
        getting_started_view_uuid.clone(),
        desktop_guide_view_uuid.clone(),
        mobile_guide_view_uuid.clone(),
        web_guide_view_uuid.clone(),
        todos_view_uuid.clone(),
      )
      .await?;

    // Create general space with 2 built-in views: Getting started, To-dos
    //    The Getting started view is a document view, and the To-dos view is a board view
    //    The Getting started view contains 2 sub views: Desktop guide, Mobile guide
    workspace_view_builder
      .with_view_builder(|view_builder| async {
        let created_at = timestamp();
        let mut view_builder = view_builder
          .with_view_id(general_view_uuid.clone())
          .with_name("General")
          .with_extra(&format!(
              "{{\"is_space\":true,\"space_icon\":\"interface_essential/home-3\",\"space_icon_color\":\"0xFFA34AFD\",\"space_permission\":0,\"space_created_at\":{}}}",
              created_at
          ));

        view_builder = view_builder.with_child_view_builder(
          |child_view_builder| async {
            let getting_started_view_uuid = getting_started_view_uuid.clone();
            let desktop_guide_view_uuid = desktop_guide_view_uuid.clone();
            let mobile_guide_view_uuid = mobile_guide_view_uuid.clone();
            let web_guide_view_uuid = web_guide_view_uuid.clone();
            let  child_view_builder = self.build_getting_started_view(child_view_builder, getting_started_view_uuid, desktop_guide_view_uuid, mobile_guide_view_uuid, web_guide_view_uuid).await;
            child_view_builder.build()
          }
        ).await;

        view_builder = view_builder.with_child_view_builder(
          |child_view_builder| async {
            let child_view_builder = child_view_builder
            .with_layout(ViewLayout::Board)
            .with_view_id(todos_view_uuid.clone())
            .with_name("To-dos")
            .with_icon("✅");
            child_view_builder.build()
          }
        ).await;

        view_builder.build()
      })
      .await;

    // Create shared space without any built-in views
    workspace_view_builder
      .with_view_builder(|view_builder| async {
        let created_at = timestamp();
        let view_builder = view_builder
        .with_view_id(shared_view_uuid.clone())
        .with_name("Shared")
        .with_extra(&format!(
            "{{\"is_space\":true,\"space_icon\":\"interface_essential/star-2\",\"space_icon_color\":\"0xFFFFBA00\",\"space_permission\":0,\"space_created_at\":{}}}",
            created_at
        ));

        view_builder.build()
      })
      .await;

    let mut template_data = vec![
      general_data,
      shared_data,
      getting_started_data,
      desktop_guide_data,
      mobile_guide_data,
      web_guide_data,
    ];
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
    let document = Document::create_with_data(collab, self.0.clone())?;
    let data = document.encode_collab()?;
    Ok(vec![TemplateData {
      template_id: TemplateObjectId::Document(object_id),
      collab_type: CollabType::Document,
      encoded_collab: data,
    }])
  }

  async fn create_workspace_view(
    &self,
    _uid: i64,
    workspace_view_builder: &mut WorkspaceViewBuilder,
  ) -> anyhow::Result<Vec<TemplateData>> {
    let view_id = gen_view_id().to_string();

    workspace_view_builder
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
pub fn replace_json_placeholders(value: &mut Value, replacements: &HashMap<String, String>) {
  match value {
    Value::String(s) => {
      if s.starts_with('<') && s.ends_with('>') {
        let key = s.trim_start_matches('<').trim_end_matches('>');
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
