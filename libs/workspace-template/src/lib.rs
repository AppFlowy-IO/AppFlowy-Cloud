use std::collections::HashMap;
use std::sync::Arc;

pub use anyhow::Result;
use async_trait::async_trait;
use collab::core::collab::{default_client_id, CollabOptions};
use collab::core::origin::CollabOrigin;
use collab::entity::EncodedCollab;

use crate::hierarchy_builder::{FlattedViews, WorkspaceViewBuilder};
use collab::preclude::Collab;
use collab_entity::CollabType;
use collab_folder::{
  timestamp, Folder, FolderData, RepeatedViewIdentifier, ViewIdentifier, ViewLayout, Workspace,
};
use uuid::Uuid;

pub mod database;
pub mod document;

pub mod hierarchy_builder;
#[cfg(test)]
mod tests;

#[async_trait]
pub trait WorkspaceTemplate {
  fn layout(&self) -> ViewLayout;

  async fn create(&self, object_id: String) -> Result<Vec<TemplateData>>;

  async fn create_workspace_view(
    &self,
    uid: i64,
    workspace_view_builder: &mut WorkspaceViewBuilder,
  ) -> Result<Vec<TemplateData>>;
}

#[derive(Clone, Debug)]
pub enum TemplateObjectId {
  Folder(String),
  Document(String),
  DatabaseRow(String),
  Database {
    object_id: String,
    // It's used to reference the database id from the object_id
    database_id: String,
  },
}

pub struct TemplateData {
  pub template_id: TemplateObjectId,
  pub collab_type: CollabType,
  pub encoded_collab: EncodedCollab,
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
  pub fn new(uid: i64, workspace_id: &Uuid) -> Self {
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
    let mut workspace_view_builder = WorkspaceViewBuilder::new(self.workspace_id.clone(), self.uid);
    let mut templates: Vec<TemplateData> = vec![];
    for handler in self.handlers.values() {
      if let Ok(template) = handler
        .create_workspace_view(self.uid, &mut workspace_view_builder)
        .await
      {
        templates.extend(template);
      }
    }

    // All views directly under the workspace should be space.
    let views = workspace_view_builder.build();
    // Safe to unwrap because we have at least one space with a document.
    let default_current_view_id = views
      .iter()
      .find(|v| !v.child_views.is_empty())
      .unwrap()
      .child_views
      .first()
      .unwrap()
      .parent_view
      .id
      .clone();

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
        uid,
        workspace,
        current_view: default_current_view_id,
        views: FlattedViews::flatten_views(views),
        favorites: Default::default(),
        recent: Default::default(),
        trash: Default::default(),
        private: Default::default(),
      };

      let options = CollabOptions::new(workspace_id.clone(), default_client_id());
      let collab = Collab::new_with_options(CollabOrigin::Empty, options)?;
      let folder = Folder::create(collab, None, folder_data);
      let data = folder.encode_collab()?;
      Ok::<_, anyhow::Error>(TemplateData {
        template_id: TemplateObjectId::Folder(workspace_id),
        collab_type: CollabType::Folder,
        encoded_collab: data,
      })
    })
    .await??;

    templates.push(folder_template);
    Ok(templates)
  }
}

pub fn gen_view_id() -> Uuid {
  uuid::Uuid::new_v4()
}
