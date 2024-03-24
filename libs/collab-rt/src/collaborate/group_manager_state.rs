use crate::collaborate::group::CollabGroup;
use crate::error::RealtimeError;

use collab_rt_entity::user::RealtimeUser;
use dashmap::mapref::one::RefMut;
use dashmap::try_result::TryResult;
use dashmap::DashMap;
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{error, event, warn};

#[derive(Default, Clone)]
pub(crate) struct GroupManagementState {
  group_by_object_id: Rc<DashMap<String, Rc<CollabGroup>>>,
  /// Keep track of all [Collab] objects that a user is subscribed to.
  editing_by_user: Arc<DashMap<RealtimeUser, HashSet<Editing>>>,
}

impl GroupManagementState {
  pub(crate) fn new() -> Self {
    Self::default()
  }

  /// Performs a periodic check to remove groups based on the following conditions:
  /// Groups that have been inactive for a specified period of time.
  pub async fn tick(&self) -> Vec<String> {
    let mut inactive_group_ids = vec![];
    for entry in self.group_by_object_id.iter() {
      let (object_id, group) = (entry.key(), entry.value());
      if group.is_inactive().await {
        inactive_group_ids.push(object_id.clone());
        if inactive_group_ids.len() > 5 {
          break;
        }
      }
    }

    if !inactive_group_ids.is_empty() {
      for object_id in &inactive_group_ids {
        self.remove_group(object_id).await;
      }
    }
    inactive_group_ids
  }

  pub async fn get_group(&self, object_id: &str) -> Option<Rc<CollabGroup>> {
    let mut attempts = 0;
    let max_attempts = 3;
    let retry_delay = Duration::from_millis(300);

    loop {
      match self.group_by_object_id.try_get(object_id) {
        TryResult::Present(group) => return Some(group.clone()),
        TryResult::Absent => return None,
        TryResult::Locked => {
          attempts += 1;
          if attempts >= max_attempts {
            warn!("Failed to get group after {} attempts", attempts);
            // Give up after exceeding the max attempts
            return None;
          }
          sleep(retry_delay).await;
        },
      }
    }
  }

  pub(crate) async fn get_mut_group(
    &self,
    object_id: &str,
  ) -> Option<RefMut<String, Rc<CollabGroup>>> {
    let mut attempts = 0;
    let max_attempts = 3;
    let retry_delay = Duration::from_millis(300);

    loop {
      match self.group_by_object_id.try_get_mut(object_id) {
        TryResult::Present(group) => return Some(group),
        TryResult::Absent => return None,
        TryResult::Locked => {
          attempts += 1;
          if attempts >= max_attempts {
            warn!("Failed to get mut group after {} attempts", attempts);
            // Give up after exceeding the max attempts
            return None;
          }
          sleep(retry_delay).await;
        },
      }
    }
  }

  pub(crate) async fn insert_group(&self, object_id: &str, group: Rc<CollabGroup>) {
    self.group_by_object_id.insert(object_id.to_string(), group);
  }

  pub(crate) async fn contains_group(&self, object_id: &str) -> bool {
    self.group_by_object_id.contains_key(object_id)
  }

  pub(crate) async fn remove_group(&self, object_id: &str) {
    let entry = self.group_by_object_id.remove(object_id);
    if let Some(entry) = entry {
      let group = entry.1;
      group.stop().await;
      group.flush_collab().await;
    } else {
      // Log error if the group doesn't exist
      error!("Group for object_id:{} not found", object_id);
    }
  }
  pub(crate) async fn insert_user(
    &self,
    user: &RealtimeUser,
    object_id: &str,
  ) -> Result<(), RealtimeError> {
    let editing = Editing {
      object_id: object_id.to_string(),
    };

    if !self.contains_group(&editing.object_id).await {
      return Err(RealtimeError::GroupNotFound(editing.object_id.clone()));
    }

    self
      .editing_by_user
      .entry(user.clone())
      .or_default()
      .insert(editing);
    Ok(())
  }

  pub(crate) async fn remove_user(&self, user: &RealtimeUser) {
    let entry = self.editing_by_user.remove(user);
    let elements = entry.map(|(_, e)| e);

    if let Some(elements) = &elements {
      for editing in elements {
        if let Some(entry) = self.group_by_object_id.get(&editing.object_id) {
          let group = entry.value();
          group.remove_user(user).await;

          if cfg!(debug_assertions) {
            event!(
              tracing::Level::TRACE,
              "{}: current group member: {}",
              &editing.object_id,
              group.user_count(),
            );
          }
        }
      }
    }
  }

  pub async fn contains_user(&self, object_id: &str, user: &RealtimeUser) -> bool {
    if let Some(entry) = self.group_by_object_id.get(object_id) {
      entry.value().contains_user(user)
    } else {
      false
    }
  }

  pub fn number_of_groups(&self) -> usize {
    self.group_by_object_id.len()
  }
}

#[derive(Debug, Hash, PartialEq, Eq, Clone)]
struct Editing {
  pub object_id: String,
}
#[cfg(test)]
mod tests {
  use crate::collaborate::group::CollabGroup;
  use crate::collaborate::group_manager_state::GroupManagementState;
  use collab::core::origin::CollabOrigin;
  use collab::preclude::Collab;
  use collab_entity::CollabType;
  use collab_rt_entity::user::RealtimeUser;
  use std::rc::Rc;

  async fn mock_group(workspace_id: &str, object_id: &str) -> Rc<CollabGroup> {
    let workspace_id = workspace_id.to_string();
    let object_id = object_id.to_string();
    let collab_type = CollabType::Document;
    let collab = Collab::new_with_origin(CollabOrigin::Server, &object_id, vec![], false);
    Rc::new(CollabGroup::new(workspace_id, object_id, collab_type, collab).await)
  }

  fn mock_user(uid: i64, device_id: &str, connect_at: i64, _object_id: &str) -> RealtimeUser {
    RealtimeUser::new(
      uid,
      device_id.to_string(),
      uuid::Uuid::new_v4().to_string(),
      connect_at,
    )
  }

  #[tokio::test]
  async fn create_group_test() {
    let state = GroupManagementState::new();

    let object_id = "1";
    let g1 = mock_group("w1", object_id).await;
    state.insert_group(object_id, g1).await;

    assert!(state.contains_group(object_id).await);
    assert_eq!(state.number_of_groups(), 1);

    let user = mock_user(1, "device_a", 1, object_id);
    state.insert_user(&user, object_id).await.unwrap();
  }

  #[tokio::test]
  async fn insert_user_but_group_not_exist_test() {
    let state = GroupManagementState::new();

    let object_id = "1";
    let user = mock_user(1, "device_a", 1, object_id);
    let err = state.insert_user(&user, object_id).await.unwrap_err();
    assert!(matches!(err, crate::error::RealtimeError::GroupNotFound(_)));
  }
}
