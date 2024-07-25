use chrono::{DateTime, Utc};
use collab_entity::{CollabType, EncodedCollab};
use collab_folder::{Folder, ViewIcon};
use database_entity::dto::{AFRole, AFWorkspaceInvitationStatus};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, ops::Deref};
use uuid::Uuid;

#[derive(Deserialize, Serialize)]
pub struct WorkspaceMembers(pub Vec<WorkspaceMember>);
#[derive(Deserialize, Serialize)]
pub struct WorkspaceMember(pub String);
impl Deref for WorkspaceMember {
  type Target = String;
  fn deref(&self) -> &Self::Target {
    &self.0
  }
}

impl From<Vec<String>> for WorkspaceMembers {
  fn from(value: Vec<String>) -> Self {
    Self(value.into_iter().map(WorkspaceMember).collect())
  }
}

#[derive(Deserialize, Serialize)]
pub struct CreateWorkspaceMembers(pub Vec<CreateWorkspaceMember>);
impl From<Vec<CreateWorkspaceMember>> for CreateWorkspaceMembers {
  fn from(value: Vec<CreateWorkspaceMember>) -> Self {
    Self(value)
  }
}

// Deprecated
#[derive(Deserialize, Serialize)]
pub struct CreateWorkspaceMember {
  pub email: String,
  pub role: AFRole,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct WorkspaceMemberInvitation {
  pub email: String,
  pub role: AFRole,
}

#[derive(Deserialize)]
pub struct WorkspaceInviteQuery {
  pub status: Option<AFWorkspaceInvitationStatus>,
}

#[derive(Deserialize, Serialize)]
pub struct WorkspaceMemberChangeset {
  pub email: String,
  pub role: Option<AFRole>,
  pub name: Option<String>,
}

impl WorkspaceMemberChangeset {
  pub fn new(email: String) -> Self {
    Self {
      email,
      role: None,
      name: None,
    }
  }
  pub fn with_role<T: Into<AFRole>>(mut self, role: T) -> Self {
    self.role = Some(role.into());
    self
  }
  pub fn with_name(mut self, name: String) -> Self {
    self.name = Some(name);
    self
  }
}

#[derive(Deserialize, Serialize)]
pub struct WorkspaceSpaceUsage {
  pub consumed_capacity: u64,
}

#[derive(Serialize, Deserialize)]
pub struct RepeatedBlobMetaData(pub Vec<BlobMetadata>);

#[derive(Serialize, Deserialize)]
pub struct BlobMetadata {
  pub workspace_id: Uuid,
  pub file_id: String,
  pub file_type: String,
  pub file_size: i64,
  pub modified_at: DateTime<Utc>,
}

#[derive(Serialize, Deserialize)]
pub struct CreateWorkspaceParam {
  pub workspace_name: Option<String>,
}

#[derive(Serialize, Deserialize, Default)]
pub struct PatchWorkspaceParam {
  pub workspace_id: Uuid,
  pub workspace_name: Option<String>,
  pub workspace_icon: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct CollabTypeParam {
  pub collab_type: CollabType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollabResponse {
  #[serde(flatten)]
  pub encode_collab: EncodedCollab,
  /// Object ID is marked with `serde(default)` to handle cases where `object_id` is missing in the data.
  /// This scenario can occur if the server data does not include `object_id` due to version downgrades (pre-0325 versions).
  /// The default ensures graceful handling of missing `object_id` during deserialization, preventing errors in client applications
  /// that expect this field to exist.
  ///
  /// We can remove this 'serde(default)' after the 0325 version is stable.
  #[serde(default)]
  pub object_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishedDuplicate {
  pub published_collab_type: CollabType,
  pub published_view_id: String,
  pub dest_view_id: String,
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct FolderView {
  pub view_id: String,
  pub name: String,
  pub icon: Option<ViewIcon>,
  pub is_space: bool,
  pub is_private: bool,
  pub extra: Option<serde_json::Value>,
  pub children: Vec<FolderView>,
}

impl From<&Folder> for FolderView {
  fn from(folder: &Folder) -> Self {
    let mut unviewable = HashSet::new();
    for private_section in folder.get_all_private_sections() {
      unviewable.insert(private_section.id);
    }
    for trash_view in folder.get_all_trash_sections() {
      unviewable.insert(trash_view.id);
    }

    let mut private_views = HashSet::new();
    for private_section in folder.get_my_private_sections() {
      unviewable.remove(&private_section.id);
      private_views.insert(private_section.id);
    }

    let workspace_id = folder.get_workspace_id();
    let root = match folder.views.get_view(&workspace_id) {
      Some(root) => root,
      None => {
        tracing::error!("failed to get root view, workspace_id: {}", workspace_id);
        return Self::default();
      },
    };

    let extra = root.extra.as_deref().map(|extra| {
      serde_json::from_str::<serde_json::Value>(extra).unwrap_or_else(|e| {
        tracing::error!("failed to parse extra field({}): {}", extra, e);
        serde_json::Value::Null
      })
    });

    Self {
      view_id: root.id.clone(),
      name: root.name.clone(),
      icon: root.icon.clone(),
      is_space: false,
      is_private: false,
      extra,
      children: root
        .children
        .iter()
        .filter(|v| !unviewable.contains(&v.id))
        .map(|v| {
          let intermediate = FolderViewIntermediate {
            folder,
            view_id: &v.id,
            unviewable: &unviewable,
            private_views: &private_views,
          };
          FolderView::from(intermediate)
        })
        .collect(),
    }
  }
}

struct FolderViewIntermediate<'a> {
  folder: &'a Folder,
  view_id: &'a str,
  unviewable: &'a HashSet<String>,
  private_views: &'a HashSet<String>,
}

impl<'a> From<FolderViewIntermediate<'a>> for FolderView {
  fn from(fv: FolderViewIntermediate) -> Self {
    let view = match fv.folder.views.get_view(fv.view_id) {
      Some(view) => view,
      None => {
        tracing::error!("failed to get view, view_id: {}", fv.view_id);
        return Self::default();
      },
    };
    let extra = view.extra.as_deref().map(|extra| {
      serde_json::from_str::<serde_json::Value>(extra).unwrap_or_else(|e| {
        tracing::error!("failed to parse extra field({}): {}", extra, e);
        serde_json::Value::Null
      })
    });

    Self {
      view_id: view.id.clone(),
      name: view.name.clone(),
      icon: view.icon.clone(),
      is_space: view_is_space(&view),
      is_private: fv.private_views.contains(&view.id),
      extra,
      children: view
        .children
        .iter()
        .filter(|v| !fv.unviewable.contains(&v.id))
        .map(|v| {
          FolderView::from(FolderViewIntermediate {
            folder: fv.folder,
            view_id: &v.id,
            unviewable: fv.unviewable,
            private_views: fv.private_views,
          })
        })
        .collect(),
    }
  }
}

fn view_is_space(view: &collab_folder::View) -> bool {
  let extra = match view.extra.as_ref() {
    Some(extra) => extra,
    None => return false,
  };
  let value = match serde_json::from_str::<serde_json::Value>(extra) {
    Ok(v) => v,
    Err(e) => {
      tracing::error!("failed to parse extra field({}): {}", extra, e);
      return false;
    },
  };
  match value.get("is_space") {
    Some(is_space_str) => is_space_str.as_bool().unwrap_or(false),
    None => false,
  }
}
