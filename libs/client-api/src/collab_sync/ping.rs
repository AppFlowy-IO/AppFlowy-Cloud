use crate::collab_sync::sink_queue::SinkQueue;
use crate::collab_sync::{DefaultMsgIdCounter, SinkSignal, SyncTimestamp};
use collab::core::origin::CollabOrigin;
use collab_rt_entity::{ClientCollabMessage, PingSync};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};
use std::time::Duration;
use tokio::sync::watch;
use tokio::time::{sleep_until, Instant};
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
    let mut next_tick = Instant::now() + duration;
    tokio::spawn(async move {
      loop {
        sleep_until(next_tick).await;

        // Set the next tick to the current time plus the duration.
        // Otherwise, it might spike the CPU usage.
        next_tick = Instant::now() + duration;

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
                if queue.is_empty() {
                  // slow down the ping sync if there are messages in the queue.
                  next_tick = Instant::now() + Duration::from_secs(30);
                } else {
                  // No need to send ping sync if there are messages in the queue.
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

                // notify the sink to proceed next message
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
