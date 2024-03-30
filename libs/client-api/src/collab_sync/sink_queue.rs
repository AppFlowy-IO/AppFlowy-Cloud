use anyhow::Error;
use collab_rt_entity::{MsgId, SinkMessage};
use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::ops::{Deref, DerefMut};
use tracing::trace;

pub(crate) struct SinkQueue<Msg> {
  queue: BinaryHeap<QueueItem<Msg>>,
}

impl<Msg> SinkQueue<Msg>
where
  Msg: SinkMessage,
{
  pub(crate) fn new() -> Self {
    Self {
      queue: Default::default(),
    }
  }

  pub(crate) fn push_msg(&mut self, msg_id: MsgId, msg: Msg) {
    #[cfg(feature = "sync_verbose_log")]
    trace!("📩 queue: {}", msg);
    self.queue.push(QueueItem::new(msg, msg_id));
  }
}

impl<Msg> Deref for SinkQueue<Msg>
where
  Msg: SinkMessage,
{
  type Target = BinaryHeap<QueueItem<Msg>>;

  fn deref(&self) -> &Self::Target {
    &self.queue
  }
}

impl<Msg> DerefMut for SinkQueue<Msg>
where
  Msg: SinkMessage,
{
  fn deref_mut(&mut self) -> &mut Self::Target {
    &mut self.queue
  }
}

#[derive(Debug, Clone)]
pub(crate) struct QueueItem<Msg> {
  inner: Msg,
  msg_id: MsgId,
}

impl<Msg> QueueItem<Msg>
where
  Msg: SinkMessage,
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
  Msg: SinkMessage,
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
