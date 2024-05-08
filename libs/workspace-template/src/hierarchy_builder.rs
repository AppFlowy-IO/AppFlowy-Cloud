use crate::gen_view_id;
use collab_folder::{
  timestamp, IconType, RepeatedViewIdentifier, View, ViewIcon, ViewIdentifier, ViewLayout,
};
use std::future::Future;

/// A builder for creating a view for a workspace.
/// The views created by this builder will be the first level views of the workspace.
pub struct WorkspaceViewBuilder {
  pub uid: i64,
  pub workspace_id: String,
  pub views: Vec<ParentChildViews>,
}

impl WorkspaceViewBuilder {
  pub fn new(workspace_id: String, uid: i64) -> Self {
    Self {
      uid,
      workspace_id,
      views: vec![],
    }
  }

  pub async fn with_view_builder<F, O>(&mut self, view_builder: F) -> String
  where
    F: Fn(ViewBuilder) -> O,
    O: Future<Output = ParentChildViews>,
  {
    let builder = ViewBuilder::new(self.uid, self.workspace_id.clone());
    let view = view_builder(builder).await;
    let view_id = view.parent_view.id.clone();
    self.views.push(view);
    view_id
  }

  pub fn build(&mut self) -> Vec<ParentChildViews> {
    std::mem::take(&mut self.views)
  }
}

/// A builder for creating a view.
/// The default layout of the view is [ViewLayout::Document]
pub struct ViewBuilder {
  uid: i64,
  parent_view_id: String,
  view_id: String,
  name: String,
  desc: String,
  layout: ViewLayout,
  child_views: Vec<ParentChildViews>,
  is_favorite: bool,
  icon: Option<ViewIcon>,
}

impl ViewBuilder {
  pub fn new(uid: i64, parent_view_id: String) -> Self {
    Self {
      uid,
      parent_view_id,
      view_id: gen_view_id().to_string(),
      name: Default::default(),
      desc: Default::default(),
      layout: ViewLayout::Document,
      child_views: vec![],
      is_favorite: false,
      icon: None,
    }
  }

  pub fn view_id(&self) -> &str {
    &self.view_id
  }

  pub fn with_layout(mut self, layout: ViewLayout) -> Self {
    self.layout = layout;
    self
  }

  pub fn with_name(mut self, name: &str) -> Self {
    self.name = name.to_string();
    self
  }

  pub fn with_desc(mut self, desc: &str) -> Self {
    self.desc = desc.to_string();
    self
  }

  pub fn with_icon(mut self, icon: &str) -> Self {
    self.icon = Some(ViewIcon {
      ty: IconType::Emoji,
      value: icon.to_string(),
    });
    self
  }

  /// Create a child view for the current view.
  /// The view created by this builder will be the next level view of the current view.
  pub async fn with_child_view_builder<F, O>(mut self, child_view_builder: F) -> Self
  where
    F: Fn(ViewBuilder) -> O,
    O: Future<Output = ParentChildViews>,
  {
    let builder = ViewBuilder::new(self.uid, self.view_id.clone());
    self.child_views.push(child_view_builder(builder).await);
    self
  }

  pub fn build(self) -> ParentChildViews {
    let view = View {
      id: self.view_id,
      parent_view_id: self.parent_view_id,
      name: self.name,
      desc: self.desc,
      created_at: timestamp(),
      is_favorite: self.is_favorite,
      layout: self.layout,
      icon: self.icon,
      created_by: Some(self.uid),
      last_edited_time: 0,
      children: RepeatedViewIdentifier::new(
        self
          .child_views
          .iter()
          .map(|v| ViewIdentifier {
            id: v.parent_view.id.clone(),
          })
          .collect(),
      ),
      last_edited_by: Some(self.uid),
      extra: None,
    };
    ParentChildViews {
      parent_view: view,
      child_views: self.child_views,
    }
  }
}

pub struct ParentChildViews {
  pub parent_view: View,
  pub child_views: Vec<ParentChildViews>,
}

pub struct FlattedViews;

impl FlattedViews {
  pub fn flatten_views(views: Vec<ParentChildViews>) -> Vec<View> {
    let mut result = vec![];
    for view in views {
      result.push(view.parent_view);
      result.append(&mut Self::flatten_views(view.child_views));
    }
    result
  }
}
