use assert_json_diff::assert_json_eq;
use collab::core::origin::CollabOrigin;
use collab::preclude::updates::decoder::Decode;
use collab::preclude::{Collab, ReadTxn, StateVector, Update};
use collab_history::biz::history::{CollabHistory, CollabSnapshot};
use serde_json::json;

#[tokio::test]
async fn encode_history_test() {
  let object_id = uuid::Uuid::new_v4().to_string();
  let history = CollabHistory::empty(&object_id).unwrap();
  let updates = update_sequence(&object_id);

  let mut snapshots = vec![];
  for update in updates {
    let snapshot = history.gen_snapshot(1).unwrap();
    history.apply_update_v2(&update).unwrap();
    snapshots.push(snapshot);
  }

  snapshots.push(history.gen_snapshot(1).unwrap());
  for (i, snapshot) in snapshots.into_iter().enumerate() {
    let json = json_from_snapshot(&history, &snapshot, &object_id);
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

fn update_sequence(object_id: &str) -> Vec<Vec<u8>> {
  let mut updates = vec![];
  let collab = Collab::new_with_origin(CollabOrigin::Empty, object_id, vec![], false);
  let mut sv = None;
  for (index, value) in ["a", "b", "c"].iter().enumerate() {
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
