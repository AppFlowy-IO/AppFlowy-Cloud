use collab_entity::CollabType;
use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::ops::{Deref, DerefMut};
use std::sync::atomic::AtomicI64;

pub(crate) struct PendingQueue {
  id_gen: AtomicI64,
  queue: BinaryHeap<PendingItem>,
}

impl PendingQueue {
  pub(crate) fn new() -> Self {
    Self {
      id_gen: Default::default(),
      queue: Default::default(),
    }
  }

  pub(crate) fn generate_item(
    &mut self,
    workspace_id: String,
    object_id: String,
    collab_type: CollabType,
  ) -> PendingItem {
    let seq = self
      .id_gen
      .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    PendingItem {
      workspace_id,
      object_id,
      seq,
      collab_type,
    }
  }

  pub(crate) fn push_item(&mut self, item: PendingItem) {
    self.queue.push(item);
  }
}

impl Deref for PendingQueue {
  type Target = BinaryHeap<PendingItem>;

  fn deref(&self) -> &Self::Target {
    &self.queue
  }
}

impl DerefMut for PendingQueue {
  fn deref_mut(&mut self) -> &mut Self::Target {
    &mut self.queue
  }
}

#[derive(Debug, Clone)]
pub(crate) struct PendingItem {
  pub(crate) workspace_id: String,
  pub(crate) object_id: String,
  pub(crate) seq: i64,
  pub(crate) collab_type: CollabType,
}

impl PartialEq<Self> for PendingItem {
  fn eq(&self, other: &Self) -> bool {
    self.object_id == other.object_id && self.seq == other.seq
  }
}

impl Eq for PendingItem {}

impl PartialOrd for PendingItem {
  fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
    Some(self.cmp(other))
  }
}

impl Ord for PendingItem {
  fn cmp(&self, other: &Self) -> Ordering {
    // smaller seq is higher priority
    self.seq.cmp(&other.seq).reverse()
  }
}
