use bytes::Bytes;
use collab::core::origin::CollabOrigin;
use collab_entity::CollabType;
use collab_rt_entity::user::UserMessage;
use collab_rt_entity::{ClientCollabMessage, CollabMessage, InitSync, MsgId};
use collab_rt_entity::{RealtimeMessage, SystemMessage};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{Read, Write};

#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize, Hash)]
pub struct AckMetaV1 {
  #[serde(rename = "sync_verbose")]
  pub verbose: String,
  pub msg_id: MsgId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
  feature = "actix_message",
  derive(actix::Message),
  rtype(result = "()")
)]
pub enum RealtimeMessageV1 {
  Collab(CollabMessage),
  User(UserMessage),
  System(SystemMessage),
}

#[test]
fn decode_0149_realtime_message_test() {
  let collab_init = read_message_from_file("migration/0149/client_init").unwrap();
  assert!(matches!(collab_init, RealtimeMessage::Collab(_)));
  if let RealtimeMessage::Collab(CollabMessage::ClientInitSync(init)) = collab_init {
    assert_eq!(init.object_id, "object id 1");
    assert_eq!(init.collab_type, CollabType::Document);
    assert_eq!(init.workspace_id, "workspace id 1");
    assert_eq!(init.msg_id, 1);
    assert_eq!(init.payload, vec![1, 2, 3, 4]);
  } else {
    panic!("Failed to decode RealtimeMessage from file");
  }

  let collab_update = read_message_from_file("migration/0149/collab_update").unwrap();
  assert!(matches!(collab_update, RealtimeMessage::Collab(_)));
  if let RealtimeMessage::Collab(CollabMessage::ClientUpdateSync(update)) = collab_update {
    assert_eq!(update.object_id, "object id 1");
    assert_eq!(update.msg_id, 10);
    assert_eq!(update.payload, Bytes::from(vec![5, 6, 7, 8]));
  } else {
    panic!("Failed to decode RealtimeMessage from file");
  }

  let client_collab_v1 = read_message_from_file("migration/0149/client_collab_v1").unwrap();
  assert!(matches!(
    client_collab_v1,
    RealtimeMessage::ClientCollabV1(_)
  ));
  if let RealtimeMessage::ClientCollabV1(messages) = client_collab_v1 {
    assert_eq!(messages.len(), 1);
    if let ClientCollabMessage::ClientUpdateSync { data } = &messages[0] {
      assert_eq!(data.object_id, "object id 1");
      assert_eq!(data.msg_id, 10);
      assert_eq!(data.payload, Bytes::from(vec![5, 6, 7, 8]));
    } else {
      panic!("Failed to decode RealtimeMessage from file");
    }
  } else {
    panic!("Failed to decode RealtimeMessage from file");
  }
}

#[test]
fn decode_0147_realtime_message_test() {
  let collab_init = read_message_from_file("migration/0147/client_init").unwrap();
  assert!(matches!(collab_init, RealtimeMessage::Collab(_)));
  if let RealtimeMessage::Collab(CollabMessage::ClientInitSync(init)) = collab_init {
    assert_eq!(init.object_id, "object id 1");
    assert_eq!(init.collab_type, CollabType::Document);
    assert_eq!(init.workspace_id, "workspace id 1");
    assert_eq!(init.msg_id, 1);
    assert_eq!(init.payload, vec![1, 2, 3, 4]);
  } else {
    panic!("Failed to decode RealtimeMessage from file");
  }

  let collab_update = read_message_from_file("migration/0147/collab_update").unwrap();
  assert!(matches!(collab_update, RealtimeMessage::Collab(_)));
  if let RealtimeMessage::Collab(CollabMessage::ClientUpdateSync(update)) = collab_update {
    assert_eq!(update.object_id, "object id 1");
    assert_eq!(update.msg_id, 10);
    assert_eq!(update.payload, Bytes::from(vec![5, 6, 7, 8]));
  } else {
    panic!("Failed to decode RealtimeMessage from file");
  }
}

#[test]
fn decode_version_2_collab_message_with_version_1_test_1() {
  let version_2 = RealtimeMessage::Collab(CollabMessage::ClientInitSync(InitSync::new(
    CollabOrigin::Empty,
    "1".to_string(),
    CollabType::Document,
    "w1".to_string(),
    1,
    vec![0u8, 3],
  )));

  let version_2_bytes = version_2.encode().unwrap();
  let version_1: RealtimeMessageV1 = bincode::deserialize(&version_2_bytes).unwrap();
  match (version_1, version_2) {
    (
      RealtimeMessageV1::Collab(CollabMessage::ClientInitSync(init_1)),
      RealtimeMessage::Collab(CollabMessage::ClientInitSync(init_2)),
    ) => {
      assert_eq!(init_1, init_2);
    },
    _ => panic!("Failed to convert RealtimeMessage2 to RealtimeMessage"),
  }
}

#[allow(dead_code)]
fn write_message_to_file(
  message: &RealtimeMessage,
  file_path: &str,
) -> Result<(), Box<dyn std::error::Error>> {
  let data = message.encode().unwrap();
  let mut file = File::create(file_path)?;
  file.write_all(&data)?;
  Ok(())
}

#[allow(dead_code)]
fn read_message_from_file(file_path: &str) -> Result<RealtimeMessage, anyhow::Error> {
  let mut file = File::open(file_path)?;
  let mut buffer = Vec::new();
  file.read_to_end(&mut buffer)?;
  let message = RealtimeMessage::decode(&buffer)?;
  Ok(message)
}
