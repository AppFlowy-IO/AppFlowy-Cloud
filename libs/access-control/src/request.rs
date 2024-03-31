use crate::access::ObjectType;
use crate::act::ActionVariant;

pub struct GroupPolicyRequest<'a> {
  pub guid: &'a str,
  pub object_type: &'a ObjectType<'a>,
  pub action: &'a ActionVariant<'a>,
}

impl GroupPolicyRequest<'_> {
  pub fn new<'a>(
    guid: &'a str,
    object_type: &'a ObjectType<'a>,
    action: &'a ActionVariant<'a>,
  ) -> GroupPolicyRequest<'a> {
    GroupPolicyRequest {
      guid,
      object_type,
      action,
    }
  }
  pub fn to_policy(&self) -> Vec<String> {
    vec![
      self.guid.to_string(),
      self.object_type.policy_object(),
      self.action.to_enforce_act().to_string(),
    ]
  }
}

pub struct WorkspacePolicyRequest<'a> {
  workspace_id: &'a str,
  uid: &'a i64,
  pub object_type: &'a ObjectType<'a>,
  pub action: &'a ActionVariant<'a>,
}

impl<'a> WorkspacePolicyRequest<'a> {
  pub fn new(
    workspace_id: &'a str,
    uid: &'a i64,
    object_type: &'a ObjectType<'a>,
    action: &'a ActionVariant<'a>,
  ) -> Self {
    Self {
      workspace_id,
      uid,
      object_type,
      action,
    }
  }

  pub fn to_policy(&self) -> Vec<String> {
    match self.object_type {
      ObjectType::Workspace(_) => {
        // If the object type is a workspace, then keep using the original object type
        vec![
          self.uid.to_string(),
          self.object_type.policy_object(),
          self.action.to_enforce_act().to_string(),
        ]
      },
      ObjectType::Collab(_) => {
        // If the object type is a collab, then convert it to a workspace object type
        let object_type = ObjectType::Workspace(self.workspace_id);
        vec![
          self.uid.to_string(),
          object_type.policy_object(),
          self.action.to_enforce_act().to_string(),
        ]
      },
    }
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

  pub fn to_policy(&self) -> Vec<String> {
    vec![
      self.uid.to_string(),
      self.object_type.policy_object(),
      self.action.to_enforce_act().to_string(),
    ]
  }
}
