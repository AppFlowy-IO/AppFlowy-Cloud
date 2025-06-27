use std::collections::{HashMap, HashSet};

use app_error::AppError;
use collab_folder::Folder;
use shared_entity::dto::workspace_dto::{PublishedView, PublishedViewInfo};

use super::folder_view::{to_dto_view_icon, to_dto_view_layout};

/// Returns only folders that are published, or one of the nested subfolders is published.
/// Exclude folders that are in the trash.
pub fn collab_folder_to_published_outline(
  root_view_id: &str,
  folder: &Folder,
  publish_view_id_to_info_map: &HashMap<String, PublishedViewInfo>,
  uid: i64,
) -> Result<PublishedView, AppError> {
  let mut unviewable = HashSet::new();
  for trash_view in folder.get_all_trash_sections(uid) {
    unviewable.insert(trash_view.id);
  }

  let max_depth = 10;
  to_publish_view(
    "",
    root_view_id,
    folder,
    &unviewable,
    publish_view_id_to_info_map,
    0,
    max_depth,
    uid,
  )
  .ok_or(AppError::InvalidPublishedOutline(format!(
    "failed to get published outline for root view id: {}",
    root_view_id
  )))
}

#[allow(clippy::too_many_arguments)]
fn to_publish_view(
  parent_view_id: &str,
  view_id: &str,
  folder: &Folder,
  unviewable: &HashSet<String>,
  publish_view_id_to_info_map: &HashMap<String, PublishedViewInfo>,
  depth: u32,
  max_depth: u32,
  uid: i64,
) -> Option<PublishedView> {
  if depth > max_depth || unviewable.contains(view_id) {
    return None;
  }

  let view = match folder.get_view(view_id, uid) {
    Some(view) => view,
    None => {
      return None;
    },
  };

  // There is currently a bug, in which the parent_view_id is not always set correctly
  if !(parent_view_id.is_empty() || view.parent_view_id == parent_view_id) {
    return None;
  }

  let extra = view.extra.as_deref().map(|extra| {
    serde_json::from_str::<serde_json::Value>(extra).unwrap_or_else(|e| {
      tracing::warn!("failed to parse extra field({}): {}", extra, e);
      serde_json::Value::Null
    })
  });
  // If pruned_view is not empty, then one or more of the children is published.
  // Hence, this view should be included in the published outline, even if it is not published itself.
  let pruned_view: Vec<PublishedView> = view
    .children
    .iter()
    .filter_map(|child_view_id| {
      to_publish_view(
        view_id,
        &child_view_id.id,
        folder,
        unviewable,
        publish_view_id_to_info_map,
        depth + 1,
        max_depth,
        uid,
      )
    })
    .collect();
  let is_published = publish_view_id_to_info_map.contains_key(view_id);
  if parent_view_id.is_empty() || is_published || !pruned_view.is_empty() {
    Some(PublishedView {
      view_id: view.id.clone(),
      name: view.name.clone(),
      icon: view
        .icon
        .as_ref()
        .map(|icon| to_dto_view_icon(icon.clone())),
      is_published,
      layout: to_dto_view_layout(&view.layout),
      extra,
      children: pruned_view,
      info: publish_view_id_to_info_map.get(view_id).cloned(),
    })
  } else {
    None
  }
}
