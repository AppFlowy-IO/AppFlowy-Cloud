use collab::core::origin::CollabOrigin;
use collab::preclude::{Collab, CollabPlugin, TransactionMut};
use collab_entity::CollabType;
use collab_stream::model::{CollabControlEvent, CollabUpdateEvent};
use parking_lot::RwLock;
use std::sync::Arc;

pub struct StreamEventMock {
  pub open_event: CollabControlEvent,
  pub close_event: CollabControlEvent,
  // each element can be decoded to a Update
  pub update_events: Vec<CollabUpdateEvent>,
}

pub async fn mock_event(workspace_id: &str, object_id: &str) -> StreamEventMock {
  let workspace_id = workspace_id.to_string();
  let object_id = object_id.to_string();
  let mut collab = Collab::new_with_origin(CollabOrigin::Empty, &object_id, vec![], true);
  let plugin = ReceiveUpdatesPlugin::default();
  collab.add_plugin(Box::new(plugin.clone()));
  collab.initialize();

  let doc_state = collab
    .encode_collab_v1(|_| Ok::<(), anyhow::Error>(()))
    .unwrap()
    .doc_state
    .to_vec();

  let open_event = CollabControlEvent::Open {
    workspace_id: workspace_id.clone(),
    object_id: object_id.clone(),
    collab_type: CollabType::Empty,
    doc_state,
  };

  let close_event = CollabControlEvent::Close {
    object_id: object_id.clone(),
  };

  for i in 0..100 {
    collab.insert(&format!("key{}", i), vec![i as u8]);
  }

  let updates = std::mem::take(&mut *plugin.updates.write());
  let update_events = updates
    .into_iter()
    .map(|update| CollabUpdateEvent::UpdateV1 {
      encode_update: update,
    })
    .collect::<Vec<_>>();

  StreamEventMock {
    open_event,
    close_event,
    update_events,
  }
}

#[derive(Clone, Default)]
struct ReceiveUpdatesPlugin {
  updates: Arc<RwLock<Vec<Vec<u8>>>>,
}

impl CollabPlugin for ReceiveUpdatesPlugin {
  fn receive_update(&self, _object_id: &str, _txn: &TransactionMut, update: &[u8]) {
    self.updates.write().push(update.to_vec());
  }
}
