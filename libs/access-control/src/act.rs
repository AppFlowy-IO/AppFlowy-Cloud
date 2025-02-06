use actix_http::Method;
use database_entity::dto::{AFAccessLevel, AFRole};
use std::cmp::Ordering;

/// Defines behavior for objects that can translate to a set of action identifiers.
///
pub trait Acts {
  fn policy_acts(&self) -> Vec<String> {
    vec![self.to_enforce_act()]
  }
  fn to_enforce_act(&self) -> String;
  fn from_enforce_act(act: &str) -> Self;
}

impl Acts for AFAccessLevel {
  /// maps different access levels to a set of predefined action
  /// identifiers that represent permissions. It starts with a base action applicable
  /// to all access levels and extends this set based on the access level:
  ///
  /// - `ReadOnly`: Only includes the base action (`"10"`), indicating minimum permissions.
  /// - `ReadAndComment`: Adds `"20"` to the base action, allowing reading and commenting.
  /// - `ReadAndWrite`: Extends permissions to include `"20"` and `"30"`, enabling reading, commenting, and writing.
  /// - `FullAccess`: Grants all possible actions by including `"20"`, `"30"`, and `"50"`, representing the highest level of access.
  ///
  fn to_enforce_act(&self) -> String {
    match self {
      AFAccessLevel::ReadOnly => "l:10".to_string(),
      AFAccessLevel::ReadAndComment => "l:20".to_string(),
      AFAccessLevel::ReadAndWrite => "l:30".to_string(),
      AFAccessLevel::FullAccess => "l:50".to_string(),
    }
  }

  fn from_enforce_act(act: &str) -> Self {
    match act {
      "l:10" => AFAccessLevel::ReadOnly,
      "l:20" => AFAccessLevel::ReadAndComment,
      "l:30" => AFAccessLevel::ReadAndWrite,
      "l:50" => AFAccessLevel::FullAccess,
      _ => AFAccessLevel::ReadOnly,
    }
  }
}

impl Acts for AFRole {
  /// Returns a vector of action identifiers associated with a user's role.
  ///
  /// The function maps each role to a set of actions, reflecting a permission hierarchy
  /// where higher roles inherit the permissions of the lower ones. The roles are defined
  /// as follows:
  /// - `Owner`: Has the highest level of access, including all possible actions (`"1"`, `"2"`, and `"3"`).
  /// - `Member`: Can perform a subset of actions allowed for owners, excluding the most privileged ones (`"2"` and `"3"`).
  /// - `Guest`: Has the least level of access, limited to the least privileged actions (`"3"` only).
  ///
  fn to_enforce_act(&self) -> String {
    match self {
      AFRole::Owner => "r:1".to_string(),
      AFRole::Member => "r:2".to_string(),
      AFRole::Guest => "r:3".to_string(),
    }
  }

  fn from_enforce_act(act: &str) -> Self {
    match act {
      "r:1" => AFRole::Owner,
      "r:2" => AFRole::Member,
      "r:3" => AFRole::Guest,
      _ => AFRole::Guest,
    }
  }
}

/// Represents the actions that can be performed on objects.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum Action {
  Read,
  Write,
  Delete,
}

impl PartialOrd for Action {
  fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
    Some(self.cmp(other))
  }
}

impl Ord for Action {
  fn cmp(&self, other: &Self) -> Ordering {
    match (self, other) {
      // Read
      (Action::Read, Action::Read) => Ordering::Equal,
      (Action::Read, _) => Ordering::Less,
      (_, Action::Read) => Ordering::Greater,
      // Write
      (Action::Write, Action::Write) => Ordering::Equal,
      (Action::Write, Action::Delete) => Ordering::Less,
      // Delete
      (Action::Delete, Action::Write) => Ordering::Greater,
      (Action::Delete, Action::Delete) => Ordering::Equal,
    }
  }
}

impl Acts for Action {
  fn to_enforce_act(&self) -> String {
    match self {
      Action::Read => "read".to_string(),
      Action::Write => "write".to_string(),
      Action::Delete => "delete".to_string(),
    }
  }

  fn from_enforce_act(act: &str) -> Self {
    match act {
      "read" => Action::Read,
      "write" => Action::Write,
      "delete" => Action::Delete,
      _ => Action::Read,
    }
  }
}

impl From<&Method> for Action {
  fn from(method: &Method) -> Self {
    match *method {
      Method::POST => Action::Write,
      Method::PUT => Action::Write,
      Method::DELETE => Action::Delete,
      _ => Action::Read,
    }
  }
}
