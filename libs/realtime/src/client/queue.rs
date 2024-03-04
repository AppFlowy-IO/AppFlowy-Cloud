use dashmap::DashMap;
use indexmap::IndexSet;
use realtime_entity::message::RealtimeMessage;

struct MessageQueue {
  messages: DashMap<String, IndexSet<RealtimeMessage>>,
}
