pub mod document;
mod hierarchy_builder;

use crate::hierarchy_builder::{FlattedViews, WorkspaceViewBuilder};
pub use anyhow::Result;
use async_trait::async_trait;
use collab::core::collab::MutexCollab;
use collab::core::origin::CollabOrigin;
use collab::entity::EncodedCollab;
use collab::preclude::Collab;
use collab_entity::CollabType;
use collab_folder::{
  timestamp, Folder, FolderData, RepeatedViewIdentifier, ViewIdentifier, ViewLayout, Workspace,
};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[async_trait]
pub trait WorkspaceTemplate {
  fn layout(&self) -> ViewLayout;

  async fn create(&self, object_id: String) -> Result<TemplateData>;

  async fn create_workspace_view(
    &self,
    uid: i64,
    workspace_view_builder: Arc<RwLock<WorkspaceViewBuilder>>,
  ) -> Result<TemplateData>;
}

pub struct TemplateData {
  pub object_id: String,
  pub object_type: CollabType,
  pub object_data: EncodedCollab,
}

pub type WorkspaceTemplateHandlers = HashMap<ViewLayout, Arc<dyn WorkspaceTemplate + Send + Sync>>;

/// A builder for creating a workspace template.
/// workspace template is a set of views that are created when a workspace is created.
pub struct WorkspaceTemplateBuilder {
  pub uid: i64,
  pub workspace_id: String,
  pub handlers: WorkspaceTemplateHandlers,
}

impl WorkspaceTemplateBuilder {
  pub fn new(uid: i64, workspace_id: &str) -> Self {
    let handlers = WorkspaceTemplateHandlers::default();
    Self {
      uid,
      workspace_id: workspace_id.to_string(),
      handlers,
    }
  }

  pub fn with_template<T>(mut self, template: T) -> Self
  where
    T: WorkspaceTemplate + Send + Sync + 'static,
  {
    self.handlers.insert(template.layout(), Arc::new(template));
    self
  }

  pub fn with_templates<T>(mut self, templates: Vec<T>) -> Self
  where
    T: WorkspaceTemplate + Send + Sync + 'static,
  {
    for template in templates {
      self.handlers.insert(template.layout(), Arc::new(template));
    }
    self
  }

  pub async fn build(&self) -> Result<Vec<TemplateData>> {
    let workspace_view_builder = Arc::new(RwLock::new(WorkspaceViewBuilder::new(
      self.workspace_id.clone(),
      self.uid,
    )));
    let mut templates = vec![];
    for handler in self.handlers.values() {
      if let Ok(template) = handler
        .create_workspace_view(self.uid, workspace_view_builder.clone())
        .await
      {
        templates.push(template);
      }
    }

    let views = workspace_view_builder.write().await.build();
    // Safe to unwrap because we have at least one view.
    let first_view = views.first().unwrap().parent_view.clone();
    let first_level_views = views
      .iter()
      .map(|value| ViewIdentifier {
        id: value.parent_view.id.clone(),
      })
      .collect::<Vec<_>>();

    let workspace = Workspace {
      id: self.workspace_id.clone(),
      name: "Workspace".to_string(),
      child_views: RepeatedViewIdentifier::new(first_level_views),
      created_at: timestamp(),
      created_by: Some(self.uid),
      last_edited_time: timestamp(),
      last_edited_by: Some(self.uid),
    };

    let uid = self.uid;
    let workspace_id = self.workspace_id.clone();
    let folder_template = tokio::task::spawn_blocking(move || {
      let folder_data = FolderData {
        workspace,
        current_view: first_view.id,
        views: FlattedViews::flatten_views(views),
        favorites: Default::default(),
        recent: Default::default(),
        trash: Default::default(),
        private: Default::default(),
      };

      let collab = Arc::new(MutexCollab::new(Collab::new_with_origin(
        CollabOrigin::Empty,
        &workspace_id,
        vec![],
        false,
      )));
      let folder = Folder::create(uid, collab, None, folder_data);
      let data = folder.encode_collab_v1()?;
      Ok::<_, anyhow::Error>(TemplateData {
        object_id: workspace_id,
        object_type: CollabType::Folder,
        object_data: data,
      })
    })
    .await??;

    templates.push(folder_template);
    Ok(templates)
  }
}

pub fn gen_view_id() -> String {
  uuid::Uuid::new_v4().to_string()
}
