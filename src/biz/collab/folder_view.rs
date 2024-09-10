use std::collections::HashSet;

use app_error::AppError;
use chrono::DateTime;
use collab_folder::{Folder, ViewLayout as CollabFolderViewLayout};
use shared_entity::dto::workspace_dto::{FolderView, ViewLayout};

/// Return all folders belonging to a workspace, excluding private sections which the user does not have access to.
pub fn collab_folder_to_folder_view(
  root_view_id: &str,
  folder: &Folder,
  max_depth: u32,
  pubished_view_ids: &HashSet<String>,
) -> Result<FolderView, AppError> {
  let mut unviewable = HashSet::new();
  for private_section in folder.get_all_private_sections() {
    unviewable.insert(private_section.id);
  }
  for trash_view in folder.get_all_trash_sections() {
    unviewable.insert(trash_view.id);
  }

  let mut private_view_ids = HashSet::new();
  for private_section in folder.get_my_private_sections() {
    unviewable.remove(&private_section.id);
    private_view_ids.insert(private_section.id);
  }

  to_folder_view(
    "",
    root_view_id,
    folder,
    &unviewable,
    &private_view_ids,
    pubished_view_ids,
    false,
    0,
    max_depth,
  )
  .ok_or(AppError::InvalidFolderView(format!(
    "There is no valid folder view belonging to the root view id: {}",
    root_view_id
  )))
}

#[allow(clippy::too_many_arguments)]
fn to_folder_view(
  parent_view_id: &str,
  view_id: &str,
  folder: &Folder,
  unviewable: &HashSet<String>,
  private_view_ids: &HashSet<String>,
  published_view_ids: &HashSet<String>,
  parent_is_private: bool,
  depth: u32,
  max_depth: u32,
) -> Option<FolderView> {
  if depth > max_depth || unviewable.contains(view_id) {
    return None;
  }

  let view = match folder.get_view(view_id) {
    Some(view) => view,
    None => {
      return None;
    },
  };

  // There is currently a bug, in which the parent_view_id is not always set correctly
  if !(parent_view_id.is_empty() || view.parent_view_id == parent_view_id) {
    return None;
  }

  let is_private =
    parent_is_private || (view_is_space(&view) && private_view_ids.contains(view_id));
  let extra = view.extra.as_deref().map(|extra| {
    serde_json::from_str::<serde_json::Value>(extra).unwrap_or_else(|e| {
      tracing::warn!("failed to parse extra field({}): {}", extra, e);
      serde_json::Value::Null
    })
  });
  let children: Vec<FolderView> = view
    .children
    .iter()
    .filter_map(|child_view_id| {
      to_folder_view(
        view_id,
        &child_view_id.id,
        folder,
        unviewable,
        private_view_ids,
        published_view_ids,
        is_private,
        depth + 1,
        max_depth,
      )
    })
    .collect();
  Some(FolderView {
    view_id: view_id.to_string(),
    name: view.name.clone(),
    icon: view
      .icon
      .as_ref()
      .map(|icon| to_dto_view_icon(icon.clone())),
    is_space: view_is_space(&view),
    is_private,
    is_published: published_view_ids.contains(view_id),
    layout: to_view_layout(&view.layout),
    created_at: DateTime::from_timestamp(view.created_at, 0).unwrap_or(DateTime::default()),
    last_edited_time: DateTime::from_timestamp(view.last_edited_time, 0)
      .unwrap_or(DateTime::default()),
    extra,
    children,
  })
}

pub fn view_is_space(view: &collab_folder::View) -> bool {
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

pub fn to_dto_view_icon(
  icon: collab_folder::ViewIcon,
) -> shared_entity::dto::workspace_dto::ViewIcon {
  shared_entity::dto::workspace_dto::ViewIcon {
    ty: to_dto_view_icon_type(icon.ty),
    value: icon.value,
  }
}

pub fn to_dto_view_icon_type(
  icon: collab_folder::IconType,
) -> shared_entity::dto::workspace_dto::IconType {
  match icon {
    collab_folder::IconType::Emoji => shared_entity::dto::workspace_dto::IconType::Emoji,
    collab_folder::IconType::Url => shared_entity::dto::workspace_dto::IconType::Url,
    collab_folder::IconType::Icon => shared_entity::dto::workspace_dto::IconType::Icon,
  }
}

pub fn to_view_layout(collab_folder_view_layout: &CollabFolderViewLayout) -> ViewLayout {
  match collab_folder_view_layout {
    CollabFolderViewLayout::Document => ViewLayout::Document,
    CollabFolderViewLayout::Grid => ViewLayout::Grid,
    CollabFolderViewLayout::Board => ViewLayout::Board,
    CollabFolderViewLayout::Calendar => ViewLayout::Calendar,
    CollabFolderViewLayout::Chat => ViewLayout::Chat,
  }
}
