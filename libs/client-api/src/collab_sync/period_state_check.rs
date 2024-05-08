use crate::collab_sync::{CollabSinkState, SinkQueue, SinkSignal};
use collab::core::origin::CollabOrigin;
use collab_rt_entity::{ClientCollabMessage, CollabStateCheck, SinkMessage};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Weak};
use std::time::Duration;
use tokio::sync::watch;
use tokio::time::{sleep_until, Instant};
use tracing::warn;

#[allow(dead_code)]
pub struct CollabStateCheckRunner;

impl CollabStateCheckRunner {
  #[allow(dead_code)]
  pub(crate) fn run(
    origin: CollabOrigin,
    object_id: String,
    message_queue: Weak<parking_lot::Mutex<SinkQueue<ClientCollabMessage>>>,
    weak_notify: Weak<watch::Sender<SinkSignal>>,
    state: Arc<CollabSinkState>,
  ) {
    let duration = if cfg!(feature = "test_fast_sync") {
      Duration::from_secs(10)
    } else {
      Duration::from_secs(20)
    };

    let mut next_tick = Instant::now() + duration;
    tokio::spawn(async move {
      loop {
        sleep_until(next_tick).await;

        // Set the next tick to the current time plus the duration.
        // Otherwise, it might spike the CPU usage.
        next_tick = Instant::now() + duration;

        match message_queue.upgrade() {
          None => {
            if cfg!(feature = "sync_verbose_log") {
              tracing::warn!("{} message queue dropped", object_id);
            }
            break;
          },
          Some(message_queue) => {
            if state.pause_ping.load(Ordering::SeqCst) {
              continue;
            } else {
              // Skip this iteration if a message was sent recently, within the specified duration.
              if !state.latest_sync.is_time_for_next_sync(duration).await {
                continue;
              }

              if let Some(mut queue) = message_queue.try_lock() {
                let is_not_empty = queue.iter().any(|item| !item.message().is_ping_sync());
                if is_not_empty {
                  if cfg!(feature = "sync_verbose_log") {
                    tracing::trace!("{} slow down check", object_id);
                  }

                  next_tick = Instant::now() + Duration::from_secs(30);
                }

                let msg_id = state.id_counter.next();
                let check = CollabStateCheck {
                  origin: origin.clone(),
                  object_id: object_id.clone(),
                  msg_id,
                };
                queue.push_msg(msg_id, ClientCollabMessage::ClientCollabStateCheck(check));

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
