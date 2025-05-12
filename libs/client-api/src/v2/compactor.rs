use crate::v2::actor::{ActionSource, WorkspaceAction};
use crate::v2::ObjectId;
use appflowy_proto::{ClientMessage, UpdateFlags};
use client_api_entity::CollabType;
use smallvec::{smallvec, SmallVec};
use tokio::sync::mpsc::UnboundedReceiver;
use yrs::updates::decoder::Decode;
use yrs::updates::encoder::Encode;

pub(super) struct ChannelReceiverCompactor {
  receiver: UnboundedReceiver<WorkspaceAction>,
  peeked: Option<WorkspaceAction>,
}

impl ChannelReceiverCompactor {
  pub(super) fn new(receiver: UnboundedReceiver<WorkspaceAction>) -> Self {
    Self {
      receiver,
      peeked: None,
    }
  }

  pub async fn recv(&mut self) -> Option<WorkspaceAction> {
    let latest = match self.peeked.take() {
      Some(action) => action,
      None => self.receiver.recv().await?,
    };
    match latest {
      WorkspaceAction::Send(
        ClientMessage::Update {
          object_id,
          collab_type,
          flags,
          update,
        },
        ActionSource::Local,
      ) => self
        .try_prefetch(object_id, collab_type, flags, update)
        .ok(),
      other => Some(other),
    }
  }

  /// Given initial [ClientMessage::Update] payload, try to prefetch (without blocking or awaiting) as
  /// many bending messages from the collab stream as possible.
  ///
  /// This is so that we can even the difference between frequent updates coming from the user with
  /// slower responding server by merging a lot of small updates together into a bigger one.
  ///
  /// Returns a compacted update message.
  fn try_prefetch(
    &mut self,
    object_id: ObjectId,
    collab_type: CollabType,
    flags: UpdateFlags,
    update: Vec<u8>,
  ) -> anyhow::Result<WorkspaceAction> {
    const SIZE_THRESHOLD: usize = 64 * 1024;
    let mut size_hint = update.len();
    let mut updates: SmallVec<[(UpdateFlags, Vec<u8>); 1]> = smallvec![(flags, update)];
    while let Ok(next) = self.receiver.try_recv() {
      match next {
        WorkspaceAction::Send(
          ClientMessage::Update {
            object_id: next_object_id,
            flags,
            update,
            ..
          },
          ActionSource::Local,
        ) if next_object_id == object_id => {
          size_hint += update.len();
          // we stack updates together until we reach a non-update message
          updates.push((flags, update));

          if size_hint >= SIZE_THRESHOLD {
            break; // potential size of the update may be over threshold, stop here and send what we have
          }
        },
        other => {
          // other type of message, we cannot compact updates anymore,
          // so we just prepend the update message and then add new one and send them
          // all together
          self.peeked = Some(other);
          break;
        },
      }
    }
    let update_count = updates.len();
    if update_count == 1 {
      let (flags, update) = std::mem::take(&mut updates[0]);
      Ok(WorkspaceAction::Send(
        ClientMessage::Update {
          object_id,
          collab_type,
          flags,
          update,
        },
        ActionSource::Local,
      ))
    } else {
      let updates = updates.into_iter().flat_map(|(flags, bytes)| match flags {
        UpdateFlags::Lib0v1 => yrs::Update::decode_v1(&bytes).ok(),
        UpdateFlags::Lib0v2 => yrs::Update::decode_v2(&bytes).ok(),
      });
      let update = yrs::Update::merge_updates(updates).encode_v1();
      tracing::debug!(
        "compacted {} updates ({} bytes) into one ({} bytes)",
        update_count,
        size_hint,
        update.len()
      );
      Ok(WorkspaceAction::Send(
        ClientMessage::Update {
          object_id,
          collab_type,
          flags: UpdateFlags::Lib0v1,
          update,
        },
        ActionSource::Local,
      ))
    }
  }
}

#[cfg(test)]
mod test {
  use crate::entity::CollabType;
  use crate::v2::actor::{ActionSource, WorkspaceAction};
  use crate::v2::compactor::ChannelReceiverCompactor;
  use appflowy_proto::{ClientMessage, UpdateFlags};
  use collab::preclude::Collab;
  use std::sync::atomic::AtomicUsize;
  use std::sync::Arc;
  use tokio::sync::mpsc::unbounded_channel;
  use uuid::Uuid;
  use yrs::updates::decoder::Decode;
  use yrs::Update;

  #[tokio::test]
  async fn update_compaction() {
    let oid = Uuid::new_v4();
    let mut c1 = Collab::new(1, oid.to_string(), "device-id", vec![], false);
    let (tx, rx) = unbounded_channel();
    let mut rx = ChannelReceiverCompactor::new(rx);
    let counter = Arc::new(AtomicUsize::new(0));
    let counter_clone = counter.clone();
    c1.doc()
      .observe_update_v1_with("test", move |_, e| {
        counter_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let action = WorkspaceAction::Send(
          ClientMessage::Update {
            object_id: oid,
            collab_type: CollabType::Unknown,
            flags: UpdateFlags::Lib0v1,
            update: e.update.clone(),
          },
          ActionSource::Local,
        );
        let _ = tx.send(action);
      })
      .unwrap();
    const UPDATE_COUNT: usize = 5;
    for i in 0..UPDATE_COUNT {
      c1.insert(&format!("key-{i}"), format!("value-{i}"));
    }
    assert_eq!(
      counter.load(std::sync::atomic::Ordering::SeqCst),
      UPDATE_COUNT
    );
    let Some(WorkspaceAction::Send(
      ClientMessage::Update { flags, update, .. },
      ActionSource::Local,
    )) = rx.recv().await
    else {
      panic!("expected Some(ClientMessage::Update)");
    };

    // we produced UPDATE_COUNT updates on C1, but only took 1 update on C2
    // this should be fine as compactor should deal with compacing pending updates
    let mut c2 = Collab::new(2, oid.to_string(), "device-id", vec![], false);
    let update = match flags {
      UpdateFlags::Lib0v1 => Update::decode_v1(&update).unwrap(),
      UpdateFlags::Lib0v2 => Update::decode_v2(&update).unwrap(),
    };
    c2.apply_update(update).unwrap();

    let state1 = c1.to_json_value();
    let state2 = c2.to_json_value();
    assert_eq!(state1, state2);
  }
}
