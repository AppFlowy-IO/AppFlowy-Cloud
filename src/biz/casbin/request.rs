use crate::biz::casbin::access_control::{ActionVariant, ObjectType, ToACAction};

pub struct WorkspacePolicyRequest<'a> {
  uid: &'a i64,
  pub object_type: ObjectType<'a>,
  pub action: &'a ActionVariant<'a>,
}

impl<'a> WorkspacePolicyRequest<'a> {
  pub fn new(workspace_id: &'a str, uid: &'a i64, action: &'a ActionVariant<'a>) -> Self {
    let object_type = ObjectType::Workspace(workspace_id);
    Self {
      uid,
      object_type,
      action,
    }
  }

  pub fn into_segments(self) -> Vec<String> {
    vec![
      self.uid.to_string(),
      self.object_type.policy_object(),
      self.action.to_action().to_string(),
    ]
  }
}

pub struct PolicyRequest<'a> {
  pub uid: i64,
  pub object_type: &'a ObjectType<'a>,
  pub action: &'a ActionVariant<'a>,
}

impl<'a> PolicyRequest<'a> {
  pub fn new(uid: i64, object_type: &'a ObjectType<'a>, action: &'a ActionVariant<'a>) -> Self {
    Self {
      uid,
      object_type,
      action,
    }
  }

  pub fn into_segments(self) -> Vec<String> {
    vec![
      self.uid.to_string(),
      self.object_type.policy_object(),
      self.action.to_action().to_string(),
    ]
  }
}
