use anyhow::Error;
use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::ops::{Deref, DerefMut};

use realtime_entity::collab_msg::{CollabSinkMessage, MsgId};
use tokio::sync::oneshot;
use tracing::{trace, warn};

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

#[derive(Debug)]
pub(crate) struct QueueItem<Msg> {
  msg: Msg,
  msg_id: MsgId,
  state: MessageState,
  tx: Option<oneshot::Sender<MsgId>>,
}

impl<Msg> QueueItem<Msg>
where
  Msg: CollabSinkMessage,
{
  pub fn new(msg: Msg, msg_id: MsgId) -> Self {
    Self {
      msg,
      msg_id,
      state: MessageState::Pending,
      tx: None,
    }
  }

  pub fn get_msg(&self) -> &Msg {
    &self.msg
  }

  pub fn state(&self) -> &MessageState {
    &self.state
  }

  pub fn set_state(&mut self, new_state: MessageState) {
    if self.state != new_state {
      self.state = new_state;

      trace!(
        "oid:{}|msg_id:{},msg state:{:?}",
        self.msg.collab_object_id(),
        self.msg_id,
        self.state
      );
    }
  }

  pub fn msg_id(&self) -> MsgId {
    self.msg_id
  }
}

impl<Msg> QueueItem<Msg>
where
  Msg: CollabSinkMessage,
{
  pub fn can_merge(&self) -> bool {
    self.msg.can_merge()
  }
  pub fn merge(&mut self, other: &Self, max_size: &usize) -> Result<bool, Error> {
    self.msg.merge(other.get_msg(), max_size)
  }
}

impl<Msg> Eq for QueueItem<Msg> where Msg: Eq {}

impl<Msg> PartialEq for QueueItem<Msg>
where
  Msg: PartialEq,
{
  fn eq(&self, other: &Self) -> bool {
    self.msg == other.msg
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
    self.msg.cmp(&other.msg)
  }
}

#[derive(Debug, Eq, PartialEq, Clone)]
pub(crate) enum MessageState {
  Pending,
  Processing,
  Done,
  Timeout,
}

impl MessageState {
  pub fn is_done(&self) -> bool {
    matches!(self, MessageState::Done)
  }
  pub fn is_processing(&self) -> bool {
    matches!(self, MessageState::Processing)
  }
}
