use anyhow::Error;
use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::ops::{Deref, DerefMut};

use realtime_entity::collab_msg::{CollabSinkMessage, MsgId};

pub(crate) struct SinkQueue<Msg> {
  #[allow(dead_code)]
  uid: i64,
  queue: BinaryHeap<QueueItem<Msg>>,
}

impl<Msg> SinkQueue<Msg>
where
  Msg: CollabSinkMessage,
{
  pub(crate) fn new(uid: i64) -> Self {
    Self {
      uid,
      queue: Default::default(),
    }
  }

  pub(crate) fn push_msg(&mut self, msg_id: MsgId, msg: Msg) {
    self.queue.push(QueueItem::new(msg, msg_id));
  }
}

impl<Msg> Deref for SinkQueue<Msg>
where
  Msg: CollabSinkMessage,
{
  type Target = BinaryHeap<QueueItem<Msg>>;

  fn deref(&self) -> &Self::Target {
    &self.queue
  }
}

impl<Msg> DerefMut for SinkQueue<Msg>
where
  Msg: CollabSinkMessage,
{
  fn deref_mut(&mut self) -> &mut Self::Target {
    &mut self.queue
  }
}

#[derive(Debug, Clone)]
pub(crate) struct QueueItem<Msg> {
  inner: Msg,
  // TODO(nathan): user inner's msg_id
  msg_id: MsgId,
}

impl<Msg> QueueItem<Msg>
where
  Msg: CollabSinkMessage,
{
  pub fn new(msg: Msg, msg_id: MsgId) -> Self {
    Self { inner: msg, msg_id }
  }

  pub fn message(&self) -> &Msg {
    &self.inner
  }

  pub fn into_message(self) -> Msg {
    self.inner
  }

  pub fn msg_id(&self) -> MsgId {
    self.msg_id
  }
}

impl<Msg> QueueItem<Msg>
where
  Msg: CollabSinkMessage,
{
  pub fn mergeable(&self) -> bool {
    self.inner.mergeable()
  }
  pub fn merge(&mut self, other: &Self, max_size: &usize) -> Result<bool, Error> {
    self.inner.merge(other.message(), max_size)
  }
}

impl<Msg> Eq for QueueItem<Msg> where Msg: Eq {}

impl<Msg> PartialEq for QueueItem<Msg>
where
  Msg: PartialEq,
{
  fn eq(&self, other: &Self) -> bool {
    self.inner == other.inner
  }
}

impl<Msg> PartialOrd for QueueItem<Msg>
where
  Msg: PartialOrd + Ord,
{
  fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
    Some(self.cmp(other))
  }
}

impl<Msg> Ord for QueueItem<Msg>
where
  Msg: Ord,
{
  fn cmp(&self, other: &Self) -> Ordering {
    self.inner.cmp(&other.inner)
  }
}
