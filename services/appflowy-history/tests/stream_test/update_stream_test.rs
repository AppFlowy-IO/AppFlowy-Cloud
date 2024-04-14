use crate::util::redis_stream;
use collab_stream::stream_group::ReadOption;

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
