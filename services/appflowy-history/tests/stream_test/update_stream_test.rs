use crate::util::redis_stream;
use collab_stream::stream_group::ReadOption;
use std::time::Duration;
use tokio::time::sleep;

#[tokio::test]
async fn single_reader_single_sender_update_stream_test() {
  let redis_stream = redis_stream().await;
  let workspace = uuid::Uuid::new_v4().to_string();
  let object_id = uuid::Uuid::new_v4().to_string();

  let mut send_group = redis_stream
    .collab_update_stream(&workspace, &object_id, "write")
    .await
    .unwrap();
  for i in 0..5 {
    send_group.insert_message(vec![i]).await.unwrap();
  }

  let mut recv_group = redis_stream
    .collab_update_stream(&workspace, &object_id, "read1")
    .await
    .unwrap();

  // the following messages are not acked so they should be pending
  // and should be returned by the next get_unacked_messages call
  let messages = recv_group
    .consumer_messages("consumer1", ReadOption::Count(2))
    .await
    .unwrap();
  assert_eq!(messages.len(), 2);
  assert_eq!(messages[0].data, vec![0]);
  assert_eq!(messages[1].data, vec![1]);
  sleep(Duration::from_secs(4)).await;

  // let pending = recv_group.get_pending().await.unwrap().unwrap();
  // assert_eq!(pending.consumers.len(), 1);
  // assert_eq!(pending.consumers[0].pending, 2);
  // let messages = recv_group
  //   .get_unacked_messages(&pending.consumers[0].name, &pending.start_id)
  //   .await
  //   .unwrap();
  // assert_eq!(messages.len(), 2);
  // assert_eq!(messages[0].data, vec![0]);
  // assert_eq!(messages[1].data, vec![1]);

  let messages = recv_group
    .consumer_messages("consumer1", ReadOption::Count(5))
    .await
    .unwrap();
  assert_eq!(messages.len(), 3);
  assert_eq!(messages[0].data, vec![2]);
  assert_eq!(messages[1].data, vec![3]);
  assert_eq!(messages[2].data, vec![4]);
}

#[tokio::test]
async fn multiple_reader_single_sender_update_stream_test() {
  let redis_stream = redis_stream().await;
  let workspace = uuid::Uuid::new_v4().to_string();
  let object_id = uuid::Uuid::new_v4().to_string();

  let mut send_group = redis_stream
    .collab_update_stream(&workspace, &object_id, "write")
    .await
    .unwrap();
  send_group.insert_message(vec![1, 2, 3]).await.unwrap();
  send_group.insert_message(vec![4, 5, 6]).await.unwrap();

  let recv_group_1 = redis_stream
    .collab_update_stream(&workspace, &object_id, "read1")
    .await
    .unwrap();

  let recv_group_2 = redis_stream
    .collab_update_stream(&workspace, &object_id, "read2")
    .await
    .unwrap();
  // Both groups should have the same messages
  for mut group in vec![recv_group_1, recv_group_2] {
    let messages = group
      .consumer_messages("consumer1", ReadOption::Count(10))
      .await
      .unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].data, vec![1, 2, 3]);
    assert_eq!(messages[1].data, vec![4, 5, 6]);
    group.ack_messages(&messages).await.unwrap();

    let messages = group
      .consumer_messages("consumer1", ReadOption::Count(10))
      .await
      .unwrap();
    assert!(messages.is_empty());
  }
}
