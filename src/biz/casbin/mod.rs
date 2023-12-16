use database_entity::dto::{AFAccessLevel, AFRole};

pub mod access_control;
pub mod adapter;
mod enforcer_ext;
pub const MODEL_CONF: &str = r###"
[request_definition]
r = sub, obj, act

[policy_definition]
p = sub, obj, act

[role_definition]
g = _, _ # role to action
g2 = _, _ # worksheet to collab

[policy_effect]
e = some(where (p.eft == allow))

[matchers]
m = r.sub == p.sub && g2(p.obj, r.obj) && g(p.act, r.act)
"###;

/// Represents the entity stored at the index of the access control policy.
/// `user_id, object_id, role/action`
///
/// E.g. user1, collab::123, Owner
const POLICY_FIELD_INDEX_USER: usize = 0;
const POLICY_FIELD_INDEX_OBJECT: usize = 1;
const POLICY_FIELD_INDEX_ACTION: usize = 2;

/// Represents the entity stored at the index of the grouping.
/// `role, action`
///
/// E.g. Owner, Write
#[allow(dead_code)]
const GROUPING_FIELD_INDEX_ROLE: usize = 0;
#[allow(dead_code)]
const GROUPING_FIELD_INDEX_ACTION: usize = 1;

/// Represents the object type that is stored in the access control policy.
#[derive(Debug)]
pub enum ObjectType<'id> {
  /// Stored as `workspace::<uuid>`
  Workspace(&'id str),
  /// Stored as `collab::<uuid>`
  Collab(&'id str),
}

impl ToString for ObjectType<'_> {
  fn to_string(&self) -> String {
    match self {
      ObjectType::Collab(s) => format!("collab::{}", s),
      ObjectType::Workspace(s) => format!("workspace::{}", s),
    }
  }
}

/// Represents the action type that is stored in the access control policy.
#[derive(Debug)]
pub enum ActionType {
  Role(AFRole),
  Level(AFAccessLevel),
}

/// Represents the actions that can be performed on objects.
#[derive(Debug)]
pub enum Action {
  Read,
  Write,
  Delete,
}

impl ToString for Action {
  fn to_string(&self) -> String {
    match self {
      Action::Read => "read".to_owned(),
      Action::Write => "write".to_owned(),
      Action::Delete => "delete".to_owned(),
    }
  }
}
