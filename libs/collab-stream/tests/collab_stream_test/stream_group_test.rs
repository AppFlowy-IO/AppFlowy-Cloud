use crate::collab_stream_test::test_util::{random_i64, stream_client};
use collab_stream::model::StreamBinary;
use collab_stream::stream_group::ReadOption;
use futures::future::join;

#[tokio::test]
async fn single_group_read_message_test() {
  let workspace_id = "w1";
  let oid = format!("o{}", random_i64());
  let client = stream_client().await;
  let mut group = client
    .collab_update_stream(workspace_id, &oid, "g1")
    .await
    .unwrap();
  let msg = StreamBinary(vec![1, 2, 3, 4, 5]);

  {
    let client = stream_client().await;
    let mut group = client
      .collab_update_stream(workspace_id, &oid, "g2")
      .await
      .unwrap();
    group.insert_binary(msg).await.unwrap();
  }

  let messages = group
    .consumer_messages("consumer1", ReadOption::Undelivered)
    .await
    .unwrap();
  assert_eq!(messages.len(), 1);
  assert_eq!(messages[0].data, vec![1, 2, 3, 4, 5]);

  // after the message was consumed, it should not be available anymore
  assert!(group
    .consumer_messages("consumer1", ReadOption::Count(1))
    .await
    .unwrap()
    .is_empty());
}

#[tokio::test]
async fn single_group_async_read_message_test() {
  let workspace_id = "w1";
  let oid = format!("o{}", random_i64());
  let client = stream_client().await;
  let mut group = client
    .collab_update_stream(workspace_id, &oid, "g1")
    .await
    .unwrap();

  let msg = StreamBinary(vec![1, 2, 3, 4, 5]);

  {
    let client = stream_client().await;
    let mut group = client
      .collab_update_stream(workspace_id, &oid, "g2")
      .await
      .unwrap();
    group.insert_binary(msg).await.unwrap();
  }

  let messages = group
    .consumer_messages("consumer1", ReadOption::Undelivered)
    .await
    .unwrap();
  assert_eq!(messages.len(), 1);
  assert_eq!(messages[0].data, vec![1, 2, 3, 4, 5]);

  // after the message was consumed, it should not be available anymore
  assert!(group
    .consumer_messages("consumer1", ReadOption::Count(1))
    .await
    .unwrap()
    .is_empty());
}

#[tokio::test]
async fn different_group_read_message_test() {
  let oid = format!("o{}", random_i64());
  let client = stream_client().await;
  let mut group_1 = client.collab_update_stream("w1", &oid, "g1").await.unwrap();
  let mut group_2 = client.collab_update_stream("w1", &oid, "g2").await.unwrap();

  let msg = StreamBinary(vec![1, 2, 3, 4, 5]);

  {
    let client = stream_client().await;
    let mut group = client.collab_update_stream("w1", &oid, "g2").await.unwrap();
    group.insert_binary(msg).await.unwrap();
  }

  let (result1, result2) = join(
    group_1.consumer_messages("consumer1", ReadOption::Undelivered),
    group_2.consumer_messages("consumer1", ReadOption::Undelivered),
  )
  .await;
  let group_1_messages = result1.unwrap();
  let group_2_messages = result2.unwrap();
  assert_eq!(group_1_messages[0].data, vec![1, 2, 3, 4, 5]);
  assert_eq!(group_2_messages[0].data, vec![1, 2, 3, 4, 5]);
}

#[tokio::test]
async fn read_specific_num_of_message_test() {
  let object_id = format!("o{}", random_i64());
  let client = stream_client().await;
  let mut group_1 = client
    .collab_update_stream("w1", &object_id, "g1")
    .await
    .unwrap();
  {
    let client = stream_client().await;
    let mut group = client
      .collab_update_stream("w1", &object_id, "g2")
      .await
      .unwrap();
    let mut messages = vec![];
    for _i in 0..5 {
      let msg = StreamBinary(vec![1, 2, 3, 4, 5]);
      messages.push(msg);
    }
    group.insert_messages(messages).await.unwrap();
  }

  let messages = group_1
    .consumer_messages("consumer1", ReadOption::Count(15))
    .await
    .unwrap();
  assert_eq!(messages.len(), 5);

  for message in messages {
    assert_eq!(
      message.data,
      vec![1, 2, 3, 4, 5],
      "Message data does not match expected pattern"
    );
  }
}

#[tokio::test]
async fn read_all_message_test() {
  let object_id = format!("o{}", random_i64());
  let client = stream_client().await;
  let mut group = client
    .collab_update_stream("w1", &object_id, "g1")
    .await
    .unwrap();
  {
    let client = stream_client().await;
    let mut group_2 = client
      .collab_update_stream("w1", &object_id, "g2")
      .await
      .unwrap();
    let mut messages = vec![];
    for _i in 0..5 {
      let msg = StreamBinary(vec![1, 2, 3, 4, 5]);
      messages.push(msg);
    }
    group_2.insert_messages(messages).await.unwrap();
  }

  // get all the message in the group
  let messages = group.get_all_message().await.unwrap();
  for message in messages {
    assert_eq!(
      message.data,
      vec![1, 2, 3, 4, 5],
      "Message data does not match expected pattern"
    );
  }
}
