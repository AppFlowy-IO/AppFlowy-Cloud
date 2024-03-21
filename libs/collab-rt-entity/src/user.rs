use collab::core::origin::CollabOrigin;
use database_entity::dto::AFWorkspaceMember;
use serde::{Deserialize, Serialize};
use std::fmt::{Debug, Display};
use std::hash::Hash;
use std::sync::Arc;

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

impl<T> From<&T> for UserDevice
where
  T: RealtimeUser,
{
  fn from(user: &T) -> Self {
    Self {
      device_id: user.device_id().to_string(),
      uid: user.uid(),
    }
  }
}

#[derive(Debug, Hash, PartialEq, Eq)]
pub struct Editing {
  pub object_id: String,
  pub origin: CollabOrigin,
}

pub trait RealtimeUser:
  Clone + Debug + Send + Sync + 'static + Display + Hash + Eq + PartialEq
{
  fn uid(&self) -> i64;
  fn device_id(&self) -> &str;
  fn user_device(&self) -> String {
    format!("{}-{}", self.uid(), self.device_id())
  }
}

impl<T> RealtimeUser for Arc<T>
where
  T: RealtimeUser,
{
  fn uid(&self) -> i64 {
    self.as_ref().uid()
  }

  fn device_id(&self) -> &str {
    self.as_ref().device_id()
  }
}
