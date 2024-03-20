use crate::stream_test::test_util::{random_i64, stream_client};
use collab_stream::model::Message;
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

  let messages = group.fetch_messages("consumer1", 1).await.unwrap();
  assert_eq!(messages.len(), 1);
  assert_eq!(messages[0].raw_data, vec![1, 2, 3, 4, 5]);
  assert_eq!(messages[0].uid, random_uid);

  // after the message was consumed, it should not be available anymore
  assert!(group
    .fetch_messages("consumer1", 1)
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
    group_1.fetch_messages("consumer1", 1),
    group_2.fetch_messages("consumer1", 1),
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

  let messages = group_1.fetch_messages("consumer1", 15).await.unwrap();
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

  let messages = group.read_all_message().await.unwrap();
  let consumer_messages = group.fetch_messages("consumer1", 15).await.unwrap();
  assert_eq!(messages.len(), 5);
  assert_eq!(consumer_messages.len(), 5);
  for i in 0..5 {
    assert_eq!(messages[i].raw_data, vec![1, 2, 3, 4, 5]);
    assert_eq!(consumer_messages[i].raw_data, vec![1, 2, 3, 4, 5]);

    assert_eq!(messages[i].uid, uids[i]);
    assert_eq!(consumer_messages[i].uid, uids[i]);
  }
}
