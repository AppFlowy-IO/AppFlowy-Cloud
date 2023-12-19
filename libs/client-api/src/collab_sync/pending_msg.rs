use anyhow::Error;
use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::ops::{Deref, DerefMut};

use realtime_entity::collab_msg::{CollabSinkMessage, MsgId};
use tokio::sync::oneshot;
use tracing::{trace, warn};

pub(crate) struct PendingMsgQueue<Msg> {
  #[allow(dead_code)]
  uid: i64,
  queue: BinaryHeap<PendingMessage<Msg>>,
}

impl<Msg> PendingMsgQueue<Msg>
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
    self.queue.push(PendingMessage::new(msg, msg_id));
  }
}

impl<Msg> Deref for PendingMsgQueue<Msg>
where
  Msg: CollabSinkMessage,
{
  type Target = BinaryHeap<PendingMessage<Msg>>;

  fn deref(&self) -> &Self::Target {
    &self.queue
  }
}

impl<Msg> DerefMut for PendingMsgQueue<Msg>
where
  Msg: CollabSinkMessage,
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

  pub fn object_id(&self) -> &str {
    self.msg.collab_object_id()
  }

  pub fn get_msg(&self) -> &Msg {
    &self.msg
  }

  pub fn state(&self) -> &MessageState {
    &self.state
  }

  pub fn set_state(&mut self, _uid: i64, new_state: MessageState) -> bool {
    if self.state != new_state {
      self.state = new_state;

      trace!(
        "oid:{}|msg_id:{},msg state:{:?}",
        self.msg.collab_object_id(),
        self.msg_id,
        self.state
      );
    }

    if self.state.is_done() {
      match self.tx.take() {
        None => false,
        Some(tx) => {
          // Notify that the message with given id was received
          match tx.send(self.msg_id) {
            Ok(_) => true,
            Err(err) => {
              warn!("Failed to send msg_id: {}, err: {}", self.msg_id, err);
              false
            },
          }
        },
      }
    } else {
      false
    }
  }

  pub fn set_ret(&mut self, tx: oneshot::Sender<MsgId>) {
    self.tx = Some(tx);
  }

  pub fn msg_id(&self) -> MsgId {
    self.msg_id
  }
}

impl<Msg> PendingMessage<Msg>
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
