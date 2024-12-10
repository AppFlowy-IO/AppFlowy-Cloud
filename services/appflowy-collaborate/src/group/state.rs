use dashmap::mapref::one::RefMut;
use dashmap::try_result::TryResult;
use dashmap::DashMap;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{error, event, trace, warn};

use crate::config::get_env_var;
use crate::error::RealtimeError;
use crate::group::group_init::CollabGroup;
use crate::metrics::CollabRealtimeMetrics;
use collab_rt_entity::user::RealtimeUser;

#[derive(Clone)]
pub(crate) struct GroupManagementState {
  group_by_object_id: Arc<DashMap<String, Arc<CollabGroup>>>,
  /// Keep track of all [Collab] objects that a user is subscribed to.
  editing_by_user: Arc<DashMap<RealtimeUser, HashSet<Editing>>>,
  metrics_calculate: Arc<CollabRealtimeMetrics>,
  /// By default, the number of groups to remove in a single batch is 50.
  remove_batch_size: usize,
}

impl GroupManagementState {
  pub(crate) fn new(metrics_calculate: Arc<CollabRealtimeMetrics>) -> Self {
    let remove_batch_size = get_env_var("APPFLOWY_COLLABORATE_REMOVE_BATCH_SIZE", "50")
      .parse::<usize>()
      .unwrap_or(50);
    Self {
      group_by_object_id: Arc::new(DashMap::new()),
      editing_by_user: Arc::new(DashMap::new()),
      metrics_calculate,
      remove_batch_size,
    }
  }

  /// Performs a periodic check to remove groups based on the following conditions:
  /// Groups that have been inactive for a specified period of time.
  pub fn get_inactive_group_ids(&self) -> Vec<String> {
    let mut inactive_group_ids = vec![];
    for entry in self.group_by_object_id.iter() {
      let (object_id, group) = (entry.key(), entry.value());
      if group.is_inactive() {
        inactive_group_ids.push(object_id.clone());
        if inactive_group_ids.len() > self.remove_batch_size {
          break;
        }
      }
    }
    if !inactive_group_ids.is_empty() {
      trace!("inactive group ids:{:?}", inactive_group_ids);
    }
    for object_id in &inactive_group_ids {
      self.remove_group(object_id);
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

  pub(crate) fn insert_group(&self, object_id: &str, group: Arc<CollabGroup>) {
    self.group_by_object_id.insert(object_id.to_string(), group);
    self.metrics_calculate.opening_collab_count.inc();
  }

  pub(crate) fn contains_group(&self, object_id: &str) -> bool {
    self.group_by_object_id.contains_key(object_id)
  }

  pub(crate) fn remove_group(&self, object_id: &str) {
    let entry = self.group_by_object_id.remove(object_id);

    if entry.is_none() {
      // Log error if the group doesn't exist
      error!("Group for object_id:{} not found", object_id);
    }
    self
      .metrics_calculate
      .opening_collab_count
      .set(self.group_by_object_id.len() as i64);
  }
  pub(crate) fn insert_user(
    &self,
    user: &RealtimeUser,
    object_id: &str,
  ) -> Result<(), RealtimeError> {
    let editing = Editing {
      object_id: object_id.to_string(),
    };

    let entry = self.editing_by_user.entry(user.clone());
    match entry {
      dashmap::mapref::entry::Entry::Occupied(_) => {},
      dashmap::mapref::entry::Entry::Vacant(_) => {
        self.metrics_calculate.num_of_editing_users.inc();
      },
    }

    entry.or_default().insert(editing);
    Ok(())
  }

  pub(crate) fn remove_user(&self, user: &RealtimeUser) {
    let entry = self.editing_by_user.remove(user);
    if entry.is_some() {
      self.metrics_calculate.num_of_editing_users.dec();
    }
    if let Some(editing_objects) = entry.map(|(_, e)| e) {
      for editing in editing_objects {
        match self.group_by_object_id.try_get(&editing.object_id) {
          TryResult::Present(group) => {
            group.remove_user(user);

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

  pub fn contains_user(&self, object_id: &str, user: &RealtimeUser) -> bool {
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
