use collab::core::origin::CollabOrigin;
use database_entity::dto::AFWorkspaceMember;
use serde::{Deserialize, Serialize};
use std::fmt::{Debug, Display, Formatter};
use std::hash::Hash;

#[derive(Debug, Clone, Serialize, Deserialize, Hash, Eq, PartialEq)]
pub enum UserMessage {
  ProfileChange(AFUserChange),
  WorkspaceMemberChange(AFWorkspaceMemberChange),
}

#[derive(Debug, Clone, Serialize, Deserialize, Hash, Eq, PartialEq)]
pub struct AFUserChange {
  pub uid: i64,
  pub name: Option<String>,
  pub email: Option<String>,
  pub metadata: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Hash, Eq, PartialEq)]
pub struct AFWorkspaceMemberChange {
  added: Vec<AFWorkspaceMember>,
  updated: Vec<AFWorkspaceMember>,
  removed: Vec<AFWorkspaceMember>,
}

#[derive(Clone, Hash, PartialEq, Eq, Debug)]
pub struct UserDevice {
  device_id: String,
  uid: i64,
}

impl UserDevice {
  pub fn new(device_id: &str, uid: i64) -> Self {
    Self {
      device_id: device_id.to_string(),
      uid,
    }
  }
}

impl From<&RealtimeUser> for UserDevice {
  fn from(user: &RealtimeUser) -> Self {
    Self {
      device_id: user.device_id.to_string(),
      uid: user.uid,
    }
  }
}

#[derive(Debug, Hash, PartialEq, Eq)]
pub struct Editing {
  pub object_id: String,
  pub origin: CollabOrigin,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct RealtimeUser {
  pub uid: i64,
  pub device_id: String,
  pub connect_at: i64,
}

impl RealtimeUser {
  pub fn new(uid: i64, device_id: String) -> Self {
    Self {
      uid,
      device_id,
      connect_at: chrono::Utc::now().timestamp(),
    }
  }

  pub fn user_device(&self) -> String {
    format!("{}:{}", self.uid, self.device_id)
  }
}

impl Display for RealtimeUser {
  fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
    f.write_fmt(format_args!(
      "uid:{}|device_id:{}|connected_at:{}",
      self.uid, self.device_id, self.connect_at,
    ))
  }
}
