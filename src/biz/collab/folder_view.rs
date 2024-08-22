use std::collections::HashSet;

use collab_folder::Folder;
use shared_entity::dto::workspace_dto::FolderView;
use uuid::Uuid;

pub fn collab_folder_to_folder_view(folder: &Folder, depth: u32) -> FolderView {
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

  let workspace_id = folder.get_workspace_id().unwrap_or_else(|| {
    tracing::error!("failed to get workspace_id");
    Uuid::nil().to_string()
  });
  let root = match folder.get_view(&workspace_id) {
    Some(root) => root,
    None => {
      tracing::error!("failed to get root view, workspace_id: {}", workspace_id);
      return FolderView::default();
    },
  };

  let extra = root.extra.as_deref().map(|extra| {
    serde_json::from_str::<serde_json::Value>(extra).unwrap_or_else(|e| {
      tracing::error!("failed to parse extra field({}): {}", extra, e);
      serde_json::Value::Null
    })
  });

  FolderView {
    view_id: root.id.clone(),
    name: root.name.clone(),
    icon: root
      .icon
      .as_ref()
      .map(|icon| to_dto_view_icon(icon.clone())),
    is_space: false,
    is_private: false,
    extra,
    children: if depth == 0 {
      vec![]
    } else {
      root
        .children
        .iter()
        .filter(|v| !unviewable.contains(&v.id))
        .map(|v| {
          let intermediate = FolderViewIntermediate {
            folder,
            view_id: &v.id,
            unviewable: &unviewable,
            private_views: &private_views,
            depth,
          };
          FolderView::from(intermediate)
        })
        .collect()
    },
  }
}

struct FolderViewIntermediate<'a> {
  folder: &'a Folder,
  view_id: &'a str,
  unviewable: &'a HashSet<String>,
  private_views: &'a HashSet<String>,
  depth: u32,
}

impl<'a> From<FolderViewIntermediate<'a>> for FolderView {
  fn from(fv: FolderViewIntermediate) -> Self {
    let view = match fv.folder.get_view(fv.view_id) {
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
      icon: view
        .icon
        .as_ref()
        .map(|icon| to_dto_view_icon(icon.clone())),
      is_space: view_is_space(&view),
      is_private: fv.private_views.contains(&view.id),
      extra,
      children: if fv.depth == 1 {
        vec![]
      } else {
        view
          .children
          .iter()
          .filter(|v| !fv.unviewable.contains(&v.id))
          .map(|v| {
            FolderView::from(FolderViewIntermediate {
              folder: fv.folder,
              view_id: &v.id,
              unviewable: fv.unviewable,
              private_views: fv.private_views,
              depth: fv.depth - 1,
            })
          })
          .collect()
      },
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

fn to_dto_view_icon(icon: collab_folder::ViewIcon) -> shared_entity::dto::workspace_dto::ViewIcon {
  shared_entity::dto::workspace_dto::ViewIcon {
    ty: to_dto_view_icon_type(icon.ty),
    value: icon.value,
  }
}

fn to_dto_view_icon_type(
  icon: collab_folder::IconType,
) -> shared_entity::dto::workspace_dto::IconType {
  match icon {
    collab_folder::IconType::Emoji => shared_entity::dto::workspace_dto::IconType::Emoji,
    collab_folder::IconType::Url => shared_entity::dto::workspace_dto::IconType::Url,
    collab_folder::IconType::Icon => shared_entity::dto::workspace_dto::IconType::Icon,
  }
}
