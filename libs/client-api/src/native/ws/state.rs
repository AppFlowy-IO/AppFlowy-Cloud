use tokio::sync::broadcast::{channel, Receiver, Sender};
use tracing::trace;
use crate::{ConnectState};

pub struct ConnectStateNotify {
  pub(crate) state: ConnectState,
  sender: Sender<ConnectState>,
}

impl ConnectStateNotify {
  pub(crate) fn new() -> Self {
    let (sender, _) = channel(100);
    Self {
      state: ConnectState::Closed,
      sender,
    }
  }

  pub(crate) fn set_state(&mut self, state: ConnectState) {
    if self.state != state {
      trace!("[websocket]: {:?}", state);
      self.state = state.clone();
      let _ = self.sender.send(state);
    }
  }

  pub(crate) fn subscribe(&self) -> Receiver<ConnectState> {
    self.sender.subscribe()
  }
}

