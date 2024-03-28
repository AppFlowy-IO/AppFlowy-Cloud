use crate::collab_sync::sink_queue::SinkQueue;
use crate::collab_sync::DefaultMsgIdCounter;
use collab::core::origin::CollabOrigin;
use collab_rt_entity::{ClientCollabMessage, PingSync};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};
use std::time::Duration;

use tokio::time::{interval, sleep};

pub struct PingSyncRunner;

impl PingSyncRunner {
  pub(crate) fn run(
    origin: CollabOrigin,
    object_id: String,
    msg_id_counter: Arc<DefaultMsgIdCounter>,
    message_queue: Weak<parking_lot::Mutex<SinkQueue<ClientCollabMessage>>>,
    pause: Arc<AtomicBool>,
  ) {
    let duration = Duration::from_secs(10);
    let interval = interval(duration);

    tokio::spawn(async move {
      sleep(duration).await;
      let mut interval = interval;
      loop {
        interval.tick().await;

        match message_queue.upgrade() {
          None => break,
          Some(message_queue) => {
            if pause.load(Ordering::SeqCst) {
              continue;
            } else if let Some(mut queue) = message_queue.try_lock() {
              let msg_id = msg_id_counter.next();
              let ping = PingSync {
                origin: origin.clone(),
                object_id: object_id.clone(),
                msg_id,
              };
              let ping = ClientCollabMessage::ClientPingSync(ping);
              queue.push_msg(msg_id, ping);
            }
          },
        }
      }
    });
  }
}
