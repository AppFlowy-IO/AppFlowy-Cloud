#[derive(Debug)]
pub enum SubjectType {
  User(i64),
  Group(String),
}

impl SubjectType {
  pub fn policy_subject(&self) -> String {
    match self {
      SubjectType::User(i) => i.to_string(),
      SubjectType::Group(s) => s.clone(),
    }
  }
}

/// Represents the object type that is stored in the access control policy.
#[derive(Debug)]
pub enum ObjectType<'id> {
  /// Stored as `workspace::<uuid>`
  Workspace(&'id str),
  /// Stored as `collab::<uuid>`
  Collab(&'id str),
}

impl ObjectType<'_> {
  pub fn policy_object(&self) -> String {
    match self {
      ObjectType::Collab(s) => format!("collab::{}", s),
      ObjectType::Workspace(s) => format!("workspace::{}", s),
    }
  }

  pub fn object_id(&self) -> &str {
    match self {
      ObjectType::Collab(s) => s,
      ObjectType::Workspace(s) => s,
    }
  }
}
