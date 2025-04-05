use crate::act::Acts;
use crate::entity::ObjectType;

pub struct PolicyRequest {
  uid: i64,
  object_type: ObjectType,
  action_policy_string: String,
}

impl PolicyRequest {
  pub fn new<T>(uid: i64, object_type: ObjectType, action: T) -> Self
  where
    T: Acts,
  {
    Self {
      uid,
      object_type,
      action_policy_string: action.to_enforce_act().to_string(),
    }
  }

  pub fn to_policy(&self) -> Vec<String> {
    vec![
      self.uid.to_string(),
      self.object_type.policy_object(),
      self.action_policy_string.clone(),
    ]
  }
}
