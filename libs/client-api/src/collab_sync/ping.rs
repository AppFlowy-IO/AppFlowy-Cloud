use crate::collab_sync::sink_queue::SinkQueue;
use crate::collab_sync::{DefaultMsgIdCounter, SinkSignal, SyncTimestamp};
use collab::core::origin::CollabOrigin;
use collab_rt_entity::{ClientCollabMessage, PingSync};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};
use std::time::Duration;
use tokio::sync::watch;
use tracing::warn;

pub struct PingSyncRunner;

impl PingSyncRunner {
  pub(crate) fn run(
    origin: CollabOrigin,
    object_id: String,
    msg_id_counter: Arc<DefaultMsgIdCounter>,
    message_queue: Weak<parking_lot::Mutex<SinkQueue<ClientCollabMessage>>>,
    pause: Arc<AtomicBool>,
    weak_notify: Weak<watch::Sender<SinkSignal>>,
    sync_timestamp: Arc<SyncTimestamp>,
  ) {
    let duration = Duration::from_secs(10);
    tokio::spawn(async move {
      let mut interval = tokio::time::interval(duration);
      loop {
        interval.tick().await;
        match message_queue.upgrade() {
          None => break,
          Some(message_queue) => {
            if pause.load(Ordering::SeqCst) {
              continue;
            } else {
              // Skip this iteration if a message was sent recently, within the specified duration.
              if !sync_timestamp.is_time_for_next_sync(duration).await {
                continue;
              }

              if let Some(mut queue) = message_queue.try_lock() {
                if !queue.is_empty() {
                  continue;
                }

                let msg_id = msg_id_counter.next();
                let ping = PingSync {
                  origin: origin.clone(),
                  object_id: object_id.clone(),
                  msg_id,
                };
                let ping = ClientCollabMessage::ClientPingSync(ping);
                queue.push_msg(msg_id, ping);

                if let Some(notify) = weak_notify.upgrade() {
                  if let Err(err) = notify.send(SinkSignal::Proceed) {
                    warn!("{} fail to send notify signal: {}", object_id, err);
                    break;
                  }
                }
              }
            }
          },
        }
      }
    });
  }
}
