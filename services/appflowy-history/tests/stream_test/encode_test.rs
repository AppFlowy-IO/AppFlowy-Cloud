use appflowy_history::biz::history::CollabHistory;
use appflowy_history::biz::snapshot::CollabSnapshot;
use appflowy_history::core::open_handle::OpenCollabHandle;
use assert_json_diff::assert_json_eq;
use collab::core::collab::DataSource;
use collab::core::origin::CollabOrigin;
use collab::preclude::updates::decoder::Decode;
use collab::preclude::{Collab, ReadTxn, StateVector, Update};
use collab_entity::CollabType;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

#[tokio::test]
async fn generate_snapshot_test() {
  let object_id = uuid::Uuid::new_v4().to_string();
  let open_collab =
    Arc::new(OpenCollabHandle::new(&object_id, vec![], CollabType::Unknown, None, None).unwrap());
  let history = &open_collab.history;

  let updates = update_sequence_for_values(
    &object_id,
    vec!["a".to_string(), "b".to_string(), "c".to_string()],
  );
  for update in updates {
    open_collab.apply_update_v1(&update).unwrap();
    sleep(Duration::from_millis(400)).await;
  }

  let ctx = history.gen_snapshot_context().await.unwrap().unwrap();
  assert_eq!(ctx.snapshots.len(), 3);

  // decode the doc_state_v2 and check if the state is correct
  let collab = Collab::new_with_source(
    CollabOrigin::Empty,
    &object_id,
    DataSource::DocStateV2(ctx.state.doc_state.clone()),
    vec![],
    true,
  )
  .unwrap();
  assert_json_eq!(
    collab.to_json_value(),
    json!({"map":{"0":"a","1":"b","2":"c"}})
  );

  for (i, snapshot) in ctx.snapshots.into_iter().enumerate() {
    let json = json_from_snapshot(history, &snapshot, &object_id);
    match i {
      0 => {
        assert_json_eq!(json, json!({"map":{"0":"a"}}));
      },
      1 => {
        assert_json_eq!(json, json!({"map":{"0":"a","1":"b"}}));
      },
      2 => {
        assert_json_eq!(json, json!({"map":{"0":"a","1":"b","2":"c"}}));
      },
      _ => unreachable!(),
    }
  }
}

#[tokio::test]
async fn snapshot_before_apply_update_test() {
  let object_id = uuid::Uuid::new_v4().to_string();
  let open_collab =
    Arc::new(OpenCollabHandle::new(&object_id, vec![], CollabType::Unknown, None, None).unwrap());
  let history = &open_collab.history;
  let updates = update_sequence_for_values(
    &object_id,
    vec!["a".to_string(), "b".to_string(), "c".to_string()],
  );

  let mut snapshots = vec![];
  for update in updates {
    // before applying the update, generate a snapshot which will be used to encode the update.
    // the snapshot update can be used to restore the state of the collab.
    let snapshot = history.gen_snapshot(1).unwrap();
    open_collab.apply_update_v1(&update).unwrap();
    snapshots.push(snapshot);
  }

  snapshots.push(history.gen_snapshot(1).unwrap());
  for (i, snapshot) in snapshots.into_iter().enumerate() {
    let json = json_from_snapshot(history, &snapshot, &object_id);
    match i {
      0 => {
        assert_json_eq!(json, json!({}));
      },
      1 => {
        assert_json_eq!(json, json!({"map":{"0":"a"}}));
      },
      2 => {
        assert_json_eq!(json, json!({"map":{"0":"a","1":"b"}}));
      },
      3 => {
        assert_json_eq!(json, json!({"map":{"0":"a","1":"b","2":"c"}}));
      },
      _ => unreachable!(),
    }
  }
}

fn json_from_snapshot(
  history: &CollabHistory,
  snapshot: &CollabSnapshot,
  object_id: &str,
) -> serde_json::Value {
  let update = Update::decode_v2(&history.encode_update_v2(snapshot).unwrap()).unwrap();
  let collab = Collab::new_with_origin(CollabOrigin::Empty, object_id, vec![], false);
  collab.with_origin_transact_mut(|txn| {
    txn.apply_update(update);
  });

  collab.to_json_value()
}

/// The update sequence is a series of updates that insert a value into a map.
fn update_sequence_for_values(object_id: &str, values: Vec<String>) -> Vec<Vec<u8>> {
  let mut updates = vec![];
  let collab = Collab::new_with_origin(CollabOrigin::Empty, object_id, vec![], false);
  let mut sv = None;
  for (index, value) in values.iter().enumerate() {
    collab.with_origin_transact_mut(|txn| {
      let map = collab.insert_map_with_txn_if_not_exist(txn, "map");
      map.insert_with_txn(txn, &index.to_string(), value.to_string());
    });

    {
      let txn = collab.transact();
      let update = txn.encode_state_as_update_v1(&sv.unwrap_or(StateVector::default()));
      updates.push(update);
    }
    sv = Some(collab.transact().state_vector());
  }
  updates
}
