use actix_http::Method;
use database_entity::dto::{AFAccessLevel, AFRole};
use redis::{ErrorKind, FromRedisValue, RedisError, RedisResult, RedisWrite, ToRedisArgs, Value};
use std::cmp::Ordering;

/// Defines behavior for objects that can translate to a set of action identifiers.
///
pub trait Acts {
  fn policy_acts(&self) -> Vec<&'static str>;
  fn to_enforce_act(&self) -> &'static str;
  fn from_enforce_act(act: &str) -> Self;
}

pub enum ActionVariant<'a> {
  FromRole(&'a AFRole),
  FromAccessLevel(&'a AFAccessLevel),
  FromAction(&'a Action),
}

impl<'a> ActionVariant<'a> {
  pub fn policy_acts(&self) -> Vec<&'static str> {
    match self {
      ActionVariant::FromRole(role) => role.policy_acts(),
      ActionVariant::FromAccessLevel(level) => level.policy_acts(),
      ActionVariant::FromAction(action) => action.policy_acts(),
    }
  }

  pub fn to_enforce_act(&self) -> &'static str {
    match self {
      ActionVariant::FromRole(role) => role.to_enforce_act(),
      ActionVariant::FromAccessLevel(level) => level.to_enforce_act(),
      ActionVariant::FromAction(action) => action.to_enforce_act(),
    }
  }
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
  fn policy_acts(&self) -> Vec<&'static str> {
    // Base action for all levels
    match self {
      AFAccessLevel::ReadOnly => vec![self.to_enforce_act()],
      AFAccessLevel::ReadAndComment => vec![self.to_enforce_act()],
      AFAccessLevel::ReadAndWrite => vec![self.to_enforce_act()],
      AFAccessLevel::FullAccess => vec![self.to_enforce_act()],
    }
  }

  fn to_enforce_act(&self) -> &'static str {
    match self {
      AFAccessLevel::ReadOnly => "l:10",
      AFAccessLevel::ReadAndComment => "l:20",
      AFAccessLevel::ReadAndWrite => "l:30",
      AFAccessLevel::FullAccess => "l:50",
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
  fn policy_acts(&self) -> Vec<&'static str> {
    match self {
      AFRole::Owner => vec![self.to_enforce_act()],
      AFRole::Member => vec![self.to_enforce_act()],
      AFRole::Guest => vec![self.to_enforce_act()],
    }
  }

  fn to_enforce_act(&self) -> &'static str {
    match self {
      AFRole::Owner => "r:1",
      AFRole::Member => "r:2",
      AFRole::Guest => "r:3",
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

impl ToRedisArgs for Action {
  fn write_redis_args<W>(&self, out: &mut W)
  where
    W: ?Sized + RedisWrite,
  {
    self.to_enforce_act().write_redis_args(out)
  }
}

impl FromRedisValue for Action {
  fn from_redis_value(v: &Value) -> RedisResult<Self> {
    let s: String = FromRedisValue::from_redis_value(v)?;
    match s.as_str() {
      "read" => Ok(Action::Read),
      "write" => Ok(Action::Write),
      "delete" => Ok(Action::Delete),
      _ => Err(RedisError::from((ErrorKind::TypeError, "invalid action"))),
    }
  }
}

impl AsRef<str> for Action {
  fn as_ref(&self) -> &str {
    self.to_enforce_act()
  }
}

impl Acts for Action {
  fn policy_acts(&self) -> Vec<&'static str> {
    match self {
      Action::Read => vec!["read"],
      Action::Write => vec!["write"],
      Action::Delete => vec!["delete"],
    }
  }

  fn to_enforce_act(&self) -> &'static str {
    match self {
      Action::Read => "read",
      Action::Write => "write",
      Action::Delete => "delete",
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
