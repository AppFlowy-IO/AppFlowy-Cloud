#[derive(Debug, Clone)]
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
#[derive(Debug, Clone)]
pub enum ObjectType {
  /// Stored as `workspace::<uuid>`
  Workspace(String),
  /// Stored as `collab::<uuid>`
  Collab(String),
}

impl ObjectType {
  pub fn policy_object(&self) -> String {
    match self {
      ObjectType::Collab(s) => format!("collab::{}", s),
      ObjectType::Workspace(s) => format!("workspace::{}", s),
    }
  }

  pub fn object_id(&self) -> String {
    match self {
      ObjectType::Collab(s) => s.clone(),
      ObjectType::Workspace(s) => s.clone(),
    }
  }
}
