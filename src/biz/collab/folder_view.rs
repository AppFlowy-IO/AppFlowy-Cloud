use std::collections::HashSet;

use app_error::AppError;
use chrono::DateTime;
use collab_folder::{Folder, SectionItem, SpacePermission, ViewLayout as CollabFolderViewLayout};
use shared_entity::dto::workspace_dto::{
  self, FavoriteFolderView, FolderView, FolderViewMinimal, RecentFolderView, TrashFolderView,
  ViewLayout,
};
use uuid::Uuid;

pub struct PrivateSpaceAndTrashViews {
  pub my_private_space_ids: HashSet<String>,
  pub other_private_space_ids: HashSet<String>,
  pub view_ids_in_trash: HashSet<String>,
}

pub fn private_space_and_trash_view_ids(folder: &Folder) -> PrivateSpaceAndTrashViews {
  let mut view_ids_in_trash = HashSet::new();
  let mut my_private_space_ids = HashSet::new();
  let mut other_private_space_ids = HashSet::new();
  for private_section in folder.get_my_private_sections() {
    match folder.get_view(&private_section.id) {
      Some(private_view) if check_if_view_is_space(&private_view) => {
        my_private_space_ids.insert(private_section.id.clone());
      },
      _ => (),
    }
  }

  for private_section in folder.get_all_private_sections() {
    match folder.get_view(&private_section.id) {
      Some(private_view)
        if check_if_view_is_space(&private_view)
          && !my_private_space_ids.contains(&private_section.id) =>
      {
        other_private_space_ids.insert(private_section.id.clone());
      },
      _ => (),
    }
  }
  for trash_view in folder.get_all_trash_sections() {
    view_ids_in_trash.insert(trash_view.id.clone());
  }
  PrivateSpaceAndTrashViews {
    my_private_space_ids,
    other_private_space_ids,
    view_ids_in_trash,
  }
}

/// Return all folders belonging to a workspace, excluding private sections which the user does not have access to.
pub fn collab_folder_to_folder_view(
  workspace_id: Uuid,
  root_view_id: &str,
  folder: &Folder,
  max_depth: u32,
  pubished_view_ids: &HashSet<String>,
) -> Result<FolderView, AppError> {
  let private_space_and_trash_view_ids = private_space_and_trash_view_ids(folder);

  to_folder_view(
    workspace_id,
    "",
    root_view_id,
    folder,
    &private_space_and_trash_view_ids,
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
  workspace_id: Uuid,
  parent_view_id: &str,
  view_id: &str,
  folder: &Folder,
  private_space_and_trash_views: &PrivateSpaceAndTrashViews,
  published_view_ids: &HashSet<String>,
  parent_is_private: bool,
  depth: u32,
  max_depth: u32,
) -> Option<FolderView> {
  let is_trash = private_space_and_trash_views
    .view_ids_in_trash
    .contains(view_id);
  let is_my_private_space = private_space_and_trash_views
    .my_private_space_ids
    .contains(view_id);
  let is_other_private_space = private_space_and_trash_views
    .other_private_space_ids
    .contains(view_id);

  if depth > max_depth || is_other_private_space || is_trash {
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

  let view_is_space = check_if_view_is_space(&view);
  // There is currently a bug, which a document that is not a space ended up as child
  // of the workspace
  let parent_is_workspace = workspace_id.to_string() == parent_view_id;
  if !view_is_space && parent_is_workspace {
    return None;
  }

  let is_private = parent_is_private || is_my_private_space;
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
        workspace_id,
        view_id,
        &child_view_id.id,
        folder,
        private_space_and_trash_views,
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
    is_space: view_is_space,
    is_private,
    is_published: published_view_ids.contains(view_id),
    layout: to_dto_view_layout(&view.layout),
    created_at: DateTime::from_timestamp(view.created_at, 0).unwrap_or_default(),
    last_edited_time: DateTime::from_timestamp(view.last_edited_time, 0).unwrap_or_default(),
    extra,
    children,
  })
}

pub fn section_items_to_favorite_folder_view(
  section_items: &[SectionItem],
  folder: &Folder,
  published_view_ids: &HashSet<String>,
) -> Vec<FavoriteFolderView> {
  section_items
    .iter()
    .filter_map(|section_item| {
      let view = folder.get_view(&section_item.id);
      view.map(|v| {
        let folder_view = FolderView {
          view_id: v.id.clone(),
          name: v.name.clone(),
          icon: v.icon.as_ref().map(|icon| to_dto_view_icon(icon.clone())),
          is_space: false,
          is_private: false,
          is_published: published_view_ids.contains(&v.id),
          created_at: DateTime::from_timestamp(v.created_at, 0).unwrap_or_default(),
          last_edited_time: DateTime::from_timestamp(v.last_edited_time, 0).unwrap_or_default(),
          layout: to_dto_view_layout(&v.layout),
          extra: v.extra.as_ref().map(|e| parse_extra_field_as_json(e)),
          children: vec![],
        };
        FavoriteFolderView {
          view: folder_view,
          favorited_at: DateTime::from_timestamp(section_item.timestamp, 0).unwrap_or_default(),
        }
      })
    })
    .collect()
}

pub fn section_items_to_recent_folder_view(
  section_items: &[SectionItem],
  folder: &Folder,
  published_view_ids: &HashSet<String>,
) -> Vec<RecentFolderView> {
  section_items
    .iter()
    .filter_map(|section_item| {
      let view = folder.get_view(&section_item.id);
      view.map(|v| {
        let folder_view = FolderView {
          view_id: v.id.clone(),
          name: v.name.clone(),
          icon: v.icon.as_ref().map(|icon| to_dto_view_icon(icon.clone())),
          is_space: false,
          is_private: false,
          is_published: published_view_ids.contains(&v.id),
          created_at: DateTime::from_timestamp(v.created_at, 0).unwrap_or_default(),
          last_edited_time: DateTime::from_timestamp(v.last_edited_time, 0).unwrap_or_default(),
          layout: to_dto_view_layout(&v.layout),
          extra: v.extra.as_ref().map(|e| parse_extra_field_as_json(e)),
          children: vec![],
        };
        RecentFolderView {
          view: folder_view,
          last_viewed_at: DateTime::from_timestamp(section_item.timestamp, 0).unwrap_or_default(),
        }
      })
    })
    .collect()
}

pub fn section_items_to_trash_folder_view(
  section_items: &[SectionItem],
  folder: &Folder,
) -> Vec<TrashFolderView> {
  section_items
    .iter()
    .filter_map(|section_item| {
      let view = folder.get_view(&section_item.id);
      view.map(|v| {
        let folder_view = FolderView {
          view_id: v.id.clone(),
          name: v.name.clone(),
          icon: v.icon.as_ref().map(|icon| to_dto_view_icon(icon.clone())),
          is_space: false,
          is_private: false,
          is_published: false,
          created_at: DateTime::from_timestamp(v.created_at, 0).unwrap_or_default(),
          last_edited_time: DateTime::from_timestamp(v.last_edited_time, 0).unwrap_or_default(),
          layout: to_dto_view_layout(&v.layout),
          extra: v.extra.as_ref().map(|e| parse_extra_field_as_json(e)),
          children: vec![],
        };
        TrashFolderView {
          view: folder_view,
          deleted_at: DateTime::from_timestamp(section_item.timestamp, 0).unwrap_or_default(),
        }
      })
    })
    .collect()
}

pub fn check_if_view_ancestors_fulfil_condition(
  view_id: &str,
  collab_folder: &Folder,
  condition: impl Fn(&collab_folder::View) -> bool,
) -> bool {
  let mut current_view_id = view_id.to_string();
  loop {
    let view = match collab_folder.get_view(&current_view_id) {
      Some(view) => view,
      None => return false,
    };
    if condition(&view) {
      return true;
    }
    current_view_id = view.parent_view_id.clone();
    if current_view_id.is_empty() || current_view_id == view.id {
      return false;
    }
  }
}

pub fn check_if_view_is_space(view: &collab_folder::View) -> bool {
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

pub fn parse_extra_field_as_json(extra: &str) -> serde_json::Value {
  serde_json::from_str::<serde_json::Value>(extra).unwrap_or_else(|e| {
    tracing::warn!("failed to parse extra field({}): {}", extra, e);
    serde_json::Value::Null
  })
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

pub fn to_dto_view_layout(collab_folder_view_layout: &CollabFolderViewLayout) -> ViewLayout {
  match collab_folder_view_layout {
    CollabFolderViewLayout::Document => ViewLayout::Document,
    CollabFolderViewLayout::Grid => ViewLayout::Grid,
    CollabFolderViewLayout::Board => ViewLayout::Board,
    CollabFolderViewLayout::Calendar => ViewLayout::Calendar,
    CollabFolderViewLayout::Chat => ViewLayout::Chat,
  }
}

pub fn to_dto_folder_view_miminal(collab_folder_view: &collab_folder::View) -> FolderViewMinimal {
  FolderViewMinimal {
    view_id: collab_folder_view.id.clone(),
    name: collab_folder_view.name.clone(),
    icon: collab_folder_view.icon.clone().map(to_dto_view_icon),
    layout: to_dto_view_layout(&collab_folder_view.layout),
  }
}

pub fn to_folder_view_icon(icon: workspace_dto::ViewIcon) -> collab_folder::ViewIcon {
  collab_folder::ViewIcon {
    ty: to_folder_view_icon_type(icon.ty),
    value: icon.value,
  }
}

pub fn to_folder_view_icon_type(icon: workspace_dto::IconType) -> collab_folder::IconType {
  match icon {
    workspace_dto::IconType::Emoji => collab_folder::IconType::Emoji,
    workspace_dto::IconType::Url => collab_folder::IconType::Url,
    workspace_dto::IconType::Icon => collab_folder::IconType::Icon,
  }
}

pub fn to_folder_view_layout(layout: workspace_dto::ViewLayout) -> collab_folder::ViewLayout {
  match layout {
    ViewLayout::Document => collab_folder::ViewLayout::Document,
    ViewLayout::Grid => collab_folder::ViewLayout::Grid,
    ViewLayout::Board => collab_folder::ViewLayout::Board,
    ViewLayout::Calendar => collab_folder::ViewLayout::Calendar,
    ViewLayout::Chat => collab_folder::ViewLayout::Chat,
  }
}

pub fn to_space_permission(space_permission: &workspace_dto::SpacePermission) -> SpacePermission {
  match space_permission {
    workspace_dto::SpacePermission::PublicToAll => SpacePermission::PublicToAll,
    workspace_dto::SpacePermission::Private => SpacePermission::Private,
  }
}
