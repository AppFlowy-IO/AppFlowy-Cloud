use crate::util::redis_stream;
use collab_entity::CollabType;
use collab_stream::model::{CollabControlEvent, Message};
use collab_stream::stream_group::ConsumeOptions;

#[tokio::test]
async fn single_reader_single_sender_test() {
  let redis_stream = redis_stream().await;
  let mut recv_group = redis_stream.collab_control_stream("recv").await.unwrap();
  let mut send_group = redis_stream.collab_control_stream("send").await.unwrap();

  let send_event = CollabControlEvent::Open {
    uid: 1,
    object_id: "object_id".to_string(),
    collab_type: CollabType::Empty,
  };
  let message: Message = send_event.clone().try_into().unwrap();
  send_group.insert_messages(vec![message]).await.unwrap();

  let message = recv_group
    .consumer_messages("consumer1", ConsumeOptions::Count(1))
    .await
    .unwrap();
  assert_eq!(message.len(), 1);
  recv_group
    .ack_messages(&[message[0].id.to_string()])
    .await
    .unwrap();
  let recv_event = CollabControlEvent::decode(&message[0].raw_data).unwrap();
  assert_eq!(send_event, recv_event);

  let messages = recv_group
    .consumer_messages("consumer1", ConsumeOptions::Count(1))
    .await
    .unwrap();
  assert!(messages.is_empty());
}

#[tokio::test]
async fn multiple_readers_single_sender_test() {
  let redis_stream = redis_stream().await;
  let mut recv_group_1 = redis_stream.collab_control_stream("recv1").await.unwrap();
  let mut recv_group_2 = redis_stream.collab_control_stream("recv2").await.unwrap();
  let mut send_group = redis_stream.collab_control_stream("send").await.unwrap();

  let send_event = CollabControlEvent::Open {
    uid: 1,
    object_id: "object_id".to_string(),
    collab_type: CollabType::Empty,
  };
  let message: Message = send_event.clone().try_into().unwrap();
  send_group.insert_messages(vec![message]).await.unwrap();

  // assert the message was received by recv_group_1 and recv_group_2
  let message = recv_group_1
    .consumer_messages("consumer1", ConsumeOptions::Count(1))
    .await
    .unwrap();
  assert_eq!(message.len(), 1);
  let recv_event = CollabControlEvent::decode(&message[0].raw_data).unwrap();
  assert_eq!(send_event, recv_event);

  let message = recv_group_2
    .consumer_messages("consumer1", ConsumeOptions::Count(1))
    .await
    .unwrap();
  assert_eq!(message.len(), 1);
  let recv_event = CollabControlEvent::decode(&message[0].raw_data).unwrap();
  assert_eq!(send_event, recv_event);
}
