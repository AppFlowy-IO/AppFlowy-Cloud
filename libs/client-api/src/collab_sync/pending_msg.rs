use collab_define::collab_msg::CollabSinkMessage;
use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::fmt::Display;
use std::ops::{Deref, DerefMut};

use crate::collab_sync::MsgId;
use tokio::sync::oneshot;

pub(crate) struct PendingMsgQueue<Msg> {
  queue: BinaryHeap<PendingMessage<Msg>>,
}

impl<Msg> PendingMsgQueue<Msg>
where
  Msg: Ord + Clone + Display,
{
  pub(crate) fn new() -> Self {
    Self {
      queue: Default::default(),
    }
  }

  pub(crate) fn push_msg(&mut self, msg_id: MsgId, msg: Msg) {
    self.queue.push(PendingMessage::new(msg, msg_id));
  }
}

impl<Msg> Deref for PendingMsgQueue<Msg>
where
  Msg: Ord,
{
  type Target = BinaryHeap<PendingMessage<Msg>>;

  fn deref(&self) -> &Self::Target {
    &self.queue
  }
}

impl<Msg> DerefMut for PendingMsgQueue<Msg>
where
  Msg: Ord,
{
  fn deref_mut(&mut self) -> &mut Self::Target {
    &mut self.queue
  }
}

#[derive(Debug)]
pub(crate) struct PendingMessage<Msg> {
  msg: Msg,
  msg_id: MsgId,
  state: MessageState,
  tx: Option<oneshot::Sender<MsgId>>,
}

impl<Msg> PendingMessage<Msg>
where
  Msg: Clone + Display,
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
    self.state = new_state;
    if self.state.is_done() && self.tx.is_some() {
      self.tx.take().map(|tx| tx.send(self.msg_id));
    }
  }

  pub fn set_ret(&mut self, tx: oneshot::Sender<MsgId>) {
    self.tx = Some(tx);
  }

  pub fn msg_id(&self) -> MsgId {
    self.msg_id
  }

  pub fn into_msg(self) -> Msg {
    self.msg
  }
}

impl<Msg> PendingMessage<Msg>
where
  Msg: CollabSinkMessage,
{
  pub fn is_mergeable(&self) -> bool {
    self.msg.mergeable()
  }

  #[allow(dead_code)]
  pub fn is_init(&self) -> bool {
    self.msg.is_init_msg()
  }

  pub fn merge(&mut self, other: Self) {
    self.msg.merge(other.into_msg());
  }
}

impl<Msg> Eq for PendingMessage<Msg> where Msg: Eq {}

impl<Msg> PartialEq for PendingMessage<Msg>
where
  Msg: PartialEq,
{
  fn eq(&self, other: &Self) -> bool {
    self.msg == other.msg
  }
}

impl<Msg> PartialOrd for PendingMessage<Msg>
where
  Msg: PartialOrd + Ord,
{
  fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
    Some(self.cmp(other))
  }
}

impl<Msg> Ord for PendingMessage<Msg>
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
