use crate::collab_stream_test::test_util::{random_i64, stream_client};
use collab_stream::model::{Message, MessageId};
use collab_stream::stream_group::ConsumeOptions;
use futures::future::join;

#[tokio::test]
async fn single_group_read_message_test() {
  let workspace_id = "w1";
  let oid = format!("o{}", random_i64());
  let client = stream_client().await;
  let mut group = client.group_stream(workspace_id, &oid, "g1").await.unwrap();

  let random_uid = random_i64();
  let msg = Message {
    uid: random_uid,
    raw_data: vec![1, 2, 3, 4, 5],
  };

  {
    let client = stream_client().await;
    let mut group = client.group_stream(workspace_id, &oid, "g2").await.unwrap();
    group.insert_message(msg).await.unwrap();
  }

  let messages = group
    .consumer_messages("consumer1", ConsumeOptions::Empty)
    .await
    .unwrap();
  assert_eq!(messages.len(), 1);
  assert_eq!(messages[0].raw_data, vec![1, 2, 3, 4, 5]);
  assert_eq!(messages[0].uid, random_uid);

  // after the message was consumed, it should not be available anymore
  assert!(group
    .consumer_messages("consumer1", ConsumeOptions::Count(1))
    .await
    .unwrap()
    .is_empty());
}

#[tokio::test]
async fn different_group_read_message_test() {
  let oid = format!("o{}", random_i64());
  let client = stream_client().await;
  let mut group_1 = client.group_stream("w1", &oid, "g1").await.unwrap();
  let mut group_2 = client.group_stream("w1", &oid, "g2").await.unwrap();

  let random_uid = random_i64();
  let msg = Message {
    uid: random_uid,
    raw_data: vec![1, 2, 3, 4, 5],
  };

  {
    let client = stream_client().await;
    let mut group = client.group_stream("w1", &oid, "g2").await.unwrap();
    group.insert_message(msg).await.unwrap();
  }

  let (result1, result2) = join(
    group_1.consumer_messages("consumer1", ConsumeOptions::Empty),
    group_2.consumer_messages("consumer1", ConsumeOptions::Empty),
  )
  .await;
  let group_1_messages = result1.unwrap();
  let group_2_messages = result2.unwrap();
  assert_eq!(group_1_messages[0].raw_data, vec![1, 2, 3, 4, 5]);
  assert_eq!(group_2_messages[0].raw_data, vec![1, 2, 3, 4, 5]);
}

#[tokio::test]
async fn read_specific_num_of_message_test() {
  let object_id = format!("o{}", random_i64());
  let client = stream_client().await;
  let mut group_1 = client.group_stream("w1", &object_id, "g1").await.unwrap();
  let mut uids = vec![];
  {
    let client = stream_client().await;
    let mut group = client.group_stream("w1", &object_id, "g2").await.unwrap();
    let mut messages = vec![];
    for _i in 0..5 {
      let random_uid = random_i64();
      uids.push(random_uid);
      let msg = Message {
        uid: random_uid,
        raw_data: vec![1, 2, 3, 4, 5],
      };
      messages.push(msg);
    }
    group.insert_messages(messages).await.unwrap();
  }

  let messages = group_1
    .consumer_messages("consumer1", ConsumeOptions::Count(15))
    .await
    .unwrap();
  assert_eq!(messages.len(), 5);
  for i in 0..5 {
    assert_eq!(messages[i].raw_data, vec![1, 2, 3, 4, 5]);
    assert_eq!(messages[i].uid, uids[i]);
  }
}

#[tokio::test]
async fn read_all_message_test() {
  let object_id = format!("o{}", random_i64());
  let client = stream_client().await;
  let mut group = client.group_stream("w1", &object_id, "g1").await.unwrap();
  let mut uids = vec![];
  {
    let client = stream_client().await;
    let mut group = client.group_stream("w1", &object_id, "g2").await.unwrap();
    let mut messages = vec![];
    for _i in 0..5 {
      let random_uid = random_i64();
      uids.push(random_uid);
      let msg = Message {
        uid: random_uid,
        raw_data: vec![1, 2, 3, 4, 5],
      };
      messages.push(msg);
    }
    group.insert_messages(messages).await.unwrap();
  }

  // get all the message in the group
  let messages = group.get_all_message().await.unwrap();
  for i in 0..5 {
    assert_eq!(messages[i].raw_data, vec![1, 2, 3, 4, 5]);
    assert_eq!(messages[i].uid, uids[i]);
  }

  // consume all message for given consumer
  let consumer_messages = group
    .consumer_messages("consumer1", ConsumeOptions::Count(15))
    .await
    .unwrap();
  assert_eq!(consumer_messages.len(), 5);
  for i in 0..5 {
    assert_eq!(messages[i].raw_data, vec![1, 2, 3, 4, 5]);
    assert_eq!(consumer_messages[i].raw_data, vec![1, 2, 3, 4, 5]);

    assert_eq!(messages[i].uid, uids[i]);
    assert_eq!(consumer_messages[i].uid, uids[i]);
  }

  // get the pending state
  let pending = group.pending_reply().await.unwrap().unwrap();
  assert_eq!(pending.consumers.len(), 1);
  assert_eq!(pending.consumers[0].name, "consumer1".to_string(),);
  assert_eq!(pending.consumers[0].pending, 5);

  // get pending message start from first message
  let mut message_id = MessageId::try_from(pending.start_id.as_str()).unwrap();

  // try to min 2 millisecond from the message id in order to get all the messages. Otherwise, only
  // 4 messages will be returned.
  message_id.sub_ms(2);
  let pending_messages = group
    .get_messages_starting_from_id(Some(message_id.to_string()), pending.count)
    .await
    .unwrap();
  assert_eq!(pending_messages.len(), 5);

  // ack all messages.
  let message_ids = consumer_messages
    .iter()
    .map(|m| m.message_id.to_string())
    .collect::<Vec<_>>();
  group.ack_messages(&message_ids).await.unwrap();
  let pending = group.pending_reply().await.unwrap();
  assert!(pending.is_none());
}
