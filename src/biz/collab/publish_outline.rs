use std::collections::HashSet;

use app_error::AppError;
use collab_folder::Folder;
use shared_entity::dto::workspace_dto::PublishedView;

use super::folder_view::{to_dto_view_icon, to_view_layout, view_is_space};

pub fn collab_folder_to_published_outline(
  folder: &Folder,
  publish_view_ids: &HashSet<String>,
) -> Result<PublishedView, AppError> {
  let mut unviewable = HashSet::new();
  for private_section in folder.get_all_private_sections() {
    unviewable.insert(private_section.id);
  }
  for trash_view in folder.get_all_trash_sections() {
    unviewable.insert(trash_view.id);
  }

  let workspace_id = folder
    .get_workspace_id()
    .ok_or_else(|| AppError::InvalidPublishedOutline("failed to get workspace_id".to_string()))?;
  let root = match folder.get_view(&workspace_id) {
    Some(root) => root,
    None => {
      return Err(AppError::InvalidPublishedOutline(
        "failed to get root view".to_string(),
      ));
    },
  };

  let extra = root.extra.as_deref().map(|extra| {
    serde_json::from_str::<serde_json::Value>(extra).unwrap_or_else(|e| {
      tracing::warn!("failed to parse extra field({}): {}", extra, e);
      serde_json::Value::Null
    })
  });

  // Set a reasonable max depth to prevent execessive recursion
  let max_depth = 10;
  let published_view = PublishedView {
    view_id: root.id.clone(),
    name: root.name.clone(),
    icon: root
      .icon
      .as_ref()
      .map(|icon| to_dto_view_icon(icon.clone())),
    layout: to_view_layout(&root.layout),
    is_published: false,
    extra,
    children: root
      .children
      .iter()
      .filter(|v| !unviewable.contains(&v.id))
      .filter_map(|v| {
        to_publish_view(
          &root.id,
          &v.id,
          folder,
          &unviewable,
          publish_view_ids,
          0,
          max_depth,
        )
      })
      .collect(),
  };
  Ok(published_view)
}

fn to_publish_view(
  parent_view_id: &str,
  view_id: &str,
  folder: &Folder,
  unviewable: &HashSet<String>,
  publish_view_ids: &HashSet<String>,
  depth: u32,
  max_depth: u32,
) -> Option<PublishedView> {
  if depth > max_depth {
    return None;
  }

  let view = match folder.get_view(view_id) {
    Some(view) => view,
    None => {
      return None;
    },
  };

  // There is currently a bug, in which the parent_view_id is not always set correctly
  if view.parent_view_id != parent_view_id {
    return None;
  }

  let extra = view.extra.as_deref().map(|extra| {
    serde_json::from_str::<serde_json::Value>(extra).unwrap_or_else(|e| {
      tracing::warn!("failed to parse extra field({}): {}", extra, e);
      serde_json::Value::Null
    })
  });
  let pruned_view: Vec<PublishedView> = view
    .children
    .iter()
    .filter(|v| !unviewable.contains(&v.id))
    .filter_map(|child_view_id| {
      to_publish_view(
        view_id,
        &child_view_id.id,
        folder,
        unviewable,
        publish_view_ids,
        depth + 1,
        max_depth,
      )
    })
    .collect();
  let is_published = publish_view_ids.contains(view_id);
  if view_is_space(&view) || is_published || !pruned_view.is_empty() {
    Some(PublishedView {
      view_id: view.id.clone(),
      name: view.name.clone(),
      icon: view
        .icon
        .as_ref()
        .map(|icon| to_dto_view_icon(icon.clone())),
      is_published,
      layout: to_view_layout(&view.layout),
      extra,
      children: pruned_view,
    })
  } else {
    None
  }
}
