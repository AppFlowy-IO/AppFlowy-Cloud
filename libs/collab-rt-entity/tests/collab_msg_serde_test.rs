use collab_rt_entity::collab_msg::{AckMeta, MsgId};
use serde::{Deserialize, Serialize};

#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize, Hash)]
pub struct AckMetaV1 {
  #[serde(rename = "sync_verbose")]
  pub verbose: String,
  pub msg_id: MsgId,
}

#[test]
fn ack_meta_serde_test() {
  let ack_meta = AckMeta {
    verbose: "123".to_string(),
    msg_id: 1,
    seq_num: 100,
  };

  let data = bincode::serialize(&ack_meta).unwrap();
  let meta = bincode::deserialize::<AckMetaV1>(&data).unwrap();
  assert_eq!(meta.verbose, ack_meta.verbose);
  assert_eq!(meta.msg_id, ack_meta.msg_id);
}
