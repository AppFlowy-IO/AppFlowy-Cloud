use dashmap::DashMap;
use indexmap::IndexSet;
use realtime_entity::message::RealtimeMessage;
use tracing::trace;

struct MessageQueue {
  messages: IndexSet<RealtimeMessage>,
}

impl MessageQueue {
  // pub fn push_message(&mut self, msg: RealtimeMessage) {
  //   if self.messages.contains(&msg) {
  //     trace!("Receive duplicate message, skip it");
  //   }
  //
  //   self.messages.insert(msg);
  // }
}
