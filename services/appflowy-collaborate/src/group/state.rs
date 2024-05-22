use crate::error::RealtimeError;
use crate::group::group_init::CollabGroup;

use crate::metrics::CollabMetricsCalculate;
use collab_rt_entity::user::RealtimeUser;
use dashmap::mapref::one::RefMut;
use dashmap::try_result::TryResult;
use dashmap::DashMap;

use std::collections::HashSet;

use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{error, event, warn};

#[derive(Default, Clone)]
pub(crate) struct GroupManagementState {
  group_by_object_id: Arc<DashMap<String, Arc<CollabGroup>>>,
  /// Keep track of all [Collab] objects that a user is subscribed to.
  editing_by_user: Arc<DashMap<RealtimeUser, HashSet<Editing>>>,
  metrics_calculate: CollabMetricsCalculate,
}

impl GroupManagementState {
  pub(crate) fn new(metrics_calculate: CollabMetricsCalculate) -> Self {
    Self {
      group_by_object_id: Arc::new(DashMap::new()),
      editing_by_user: Arc::new(DashMap::new()),
      metrics_calculate,
    }
  }

  /// Performs a periodic check to remove groups based on the following conditions:
  /// Groups that have been inactive for a specified period of time.
  pub async fn get_inactive_group_ids(&self) -> Vec<String> {
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

  pub async fn get_group(&self, object_id: &str) -> Option<Arc<CollabGroup>> {
    let mut attempts = 0;
    let max_attempts = 3;
    let retry_delay = Duration::from_millis(100);

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

  /// Get a mutable reference to the group by object_id.
  /// may deadlock when holding the RefMut and trying to read group_by_object_id.
  pub(crate) async fn get_mut_group(
    &self,
    object_id: &str,
  ) -> Option<RefMut<String, Arc<CollabGroup>>> {
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

  pub(crate) async fn insert_group(&self, object_id: &str, group: Arc<CollabGroup>) {
    self.group_by_object_id.insert(object_id.to_string(), group);
    self
      .metrics_calculate
      .num_of_active_collab
      .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
  }

  pub(crate) async fn contains_group(&self, object_id: &str) -> bool {
    self.group_by_object_id.contains_key(object_id)
  }

  pub(crate) async fn remove_group(&self, object_id: &str) {
    let entry = self.group_by_object_id.remove(object_id);
    if let Some(entry) = entry {
      let group = entry.1;
      group.stop().await;
    } else {
      // Log error if the group doesn't exist
      error!("Group for object_id:{} not found", object_id);
    }

    self.metrics_calculate.num_of_active_collab.store(
      self.group_by_object_id.len() as i64,
      std::sync::atomic::Ordering::Relaxed,
    );
  }
  pub(crate) async fn insert_user(
    &self,
    user: &RealtimeUser,
    object_id: &str,
  ) -> Result<(), RealtimeError> {
    let editing = Editing {
      object_id: object_id.to_string(),
    };

    self
      .editing_by_user
      .entry(user.clone())
      .or_default()
      .insert(editing);
    Ok(())
  }

  pub(crate) async fn remove_user(&self, user: &RealtimeUser) {
    let entry = self.editing_by_user.remove(user);
    if let Some(editing_objects) = entry.map(|(_, e)| e) {
      for editing in editing_objects {
        match self.group_by_object_id.try_get(&editing.object_id) {
          TryResult::Present(group) => {
            group.remove_user(user).await;

            if cfg!(debug_assertions) {
              event!(
                tracing::Level::TRACE,
                "{}: current group member: {}",
                &editing.object_id,
                group.user_count(),
              );
            }
          },
          TryResult::Absent => {},
          TryResult::Locked => {
            error!(
              "Failed to get the group:{}. cause by lock issue",
              editing.object_id
            );
          },
        }
      }
    }
  }

  pub async fn contains_user(&self, object_id: &str, user: &RealtimeUser) -> bool {
    match self.group_by_object_id.try_get(object_id) {
      TryResult::Present(entry) => entry.value().contains_user(user),
      TryResult::Absent => false,
      TryResult::Locked => {
        error!("Failed to get the group:{}. cause by lock issue", object_id);
        false
      },
    }
  }
}

#[derive(Debug, Hash, PartialEq, Eq, Clone)]
struct Editing {
  pub object_id: String,
}
