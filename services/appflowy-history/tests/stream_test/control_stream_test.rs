use crate::util::redis_stream;
use collab_entity::CollabType;
use collab_stream::model::{CollabControlEvent, StreamBinary};
use collab_stream::stream_group::ReadOption;
use serial_test::serial;
use std::time::Duration;
use tokio::time::sleep;

#[tokio::test]
#[serial]
async fn single_reader_single_sender_test() {
  let control_stream_key = uuid::Uuid::new_v4().to_string();
  let redis_stream = redis_stream().await;
  let recv_group_name = format!("recv-{}", uuid::Uuid::new_v4());
  let send_group_name = format!("send-{}", uuid::Uuid::new_v4());
  let mut recv_group = redis_stream
    .collab_control_stream(&control_stream_key, &recv_group_name)
    .await
    .unwrap();
  // clear before starting the test. Otherwise, the receive group may have messages from previous tests
  recv_group.clear().await.unwrap();

  let mut send_group = redis_stream
    .collab_control_stream(&control_stream_key, &send_group_name)
    .await
    .unwrap();

  let send_event = mock_event("object_id".to_string());
  let message: StreamBinary = send_event.clone().try_into().unwrap();
  send_group.insert_messages(vec![message]).await.unwrap();

  let messages = recv_group
    .consumer_messages("consumer1", ReadOption::Count(10))
    .await
    .unwrap();
  assert_eq!(messages.len(), 1);
  recv_group.ack_messages(&messages).await.unwrap();

  let recv_event = CollabControlEvent::decode(&messages[0].data).unwrap();
  assert_eq!(send_event, recv_event);

  let messages = recv_group
    .consumer_messages("consumer1", ReadOption::Count(10))
    .await
    .unwrap();
  assert!(
    messages.is_empty(),
    "No more messages should be available, but got {}",
    messages.len()
  );
}

#[tokio::test]
#[serial]
async fn multiple_readers_single_sender_test() {
  let control_stream_key = uuid::Uuid::new_v4().to_string();
  let redis_stream = redis_stream().await;
  let recv_group_1 = format!("recv-{}", uuid::Uuid::new_v4());
  let recv_group_2 = format!("recv-{}", uuid::Uuid::new_v4());
  let send_group = format!("send-{}", uuid::Uuid::new_v4());
  let mut recv_group_1 = redis_stream
    .collab_control_stream(&control_stream_key, &recv_group_1)
    .await
    .unwrap();
  recv_group_1.clear().await.unwrap();

  let mut recv_group_2 = redis_stream
    .collab_control_stream(&control_stream_key, &recv_group_2)
    .await
    .unwrap();
  recv_group_2.clear().await.unwrap();

  let mut send_group = redis_stream
    .collab_control_stream(&control_stream_key, &send_group)
    .await
    .unwrap();

  let send_event = mock_event("object_id".to_string());
  let message: StreamBinary = send_event.clone().try_into().unwrap();
  send_group.insert_messages(vec![message]).await.unwrap();

  // assert the message was received by recv_group_1 and recv_group_2
  let message = recv_group_1
    .consumer_messages("consumer1", ReadOption::Count(1))
    .await
    .unwrap();
  assert_eq!(message.len(), 1);
  let recv_event = CollabControlEvent::decode(&message[0].data).unwrap();
  assert_eq!(send_event, recv_event);

  let message = recv_group_2
    .consumer_messages("consumer1", ReadOption::Count(1))
    .await
    .unwrap();
  assert_eq!(message.len(), 1);
  let recv_event = CollabControlEvent::decode(&message[0].data).unwrap();
  assert_eq!(send_event, recv_event);
}

#[tokio::test]
#[serial]
async fn reading_pending_event_test() {
  let control_stream_key = uuid::Uuid::new_v4().to_string();
  let redis_stream = redis_stream().await;
  let send_group_name = format!("send-{}", uuid::Uuid::new_v4());
  let mut send_group = redis_stream
    .collab_control_stream(&control_stream_key, &send_group_name)
    .await
    .unwrap();
  let send_event = mock_event("object_id".to_string());
  let message: StreamBinary = send_event.clone().try_into().unwrap();
  send_group.insert_messages(vec![message]).await.unwrap();

  let recv_group_name = format!("recv-{}", uuid::Uuid::new_v4());
  let mut recv_group = redis_stream
    .collab_control_stream(&control_stream_key, &recv_group_name)
    .await
    .unwrap();

  // recv_group will read all non-acknowledged messages. At least one message that was inserted by send_group should be there
  let messages = recv_group
    .consumer_messages("consumer1", ReadOption::Undelivered)
    .await
    .unwrap();
  recv_group.ack_messages(&messages).await.unwrap();
  assert!(!messages.is_empty());

  let messages = recv_group
    .consumer_messages("consumer1", ReadOption::Count(1))
    .await
    .unwrap();
  assert!(messages.is_empty());
}

#[tokio::test]
#[serial]
async fn ack_partial_message_test() {
  let control_stream_key = uuid::Uuid::new_v4().to_string();
  let redis_stream = redis_stream().await;
  let send_group_name = format!("send-{}", uuid::Uuid::new_v4());
  let mut send_group = redis_stream
    .collab_control_stream(&control_stream_key, &send_group_name)
    .await
    .unwrap();
  let recv_group_name = format!("recv-{}", uuid::Uuid::new_v4());
  let mut recv_group = redis_stream
    .collab_control_stream(&control_stream_key, &recv_group_name)
    .await
    .unwrap();
  recv_group.clear().await.unwrap();

  for i in 0..3 {
    let send_event = mock_event(i.to_string());
    let message: StreamBinary = send_event.clone().try_into().unwrap();
    send_group.insert_messages(vec![message]).await.unwrap();
  }

  // recv_group will read all non-acknowledged messages. At least one message that was inserted by send_group should be there
  let mut messages = recv_group
    .consumer_messages("consumer1", ReadOption::Undelivered)
    .await
    .unwrap();
  assert_eq!(messages.len(), 3);

  // simulate the last message is not acked
  messages.pop();
  recv_group.ack_messages(&messages).await.unwrap();

  // sleep for a while to make sure the message is considered as pending
  sleep(Duration::from_secs(2)).await;

  let pending = recv_group.get_pending().await.unwrap().unwrap();
  assert_eq!(pending.consumers.len(), 1);
  assert_eq!(pending.consumers[0].pending, 1);
  let messages = recv_group
    .get_unacked_messages_with_range(
      &pending.consumers[0].name,
      &pending.start_id,
      &pending.end_id,
    )
    .await
    .unwrap();
  assert_eq!(messages.len(), 1);

  let recv_event = CollabControlEvent::decode(&messages[0].data).unwrap();
  assert_eq!(
    CollabControlEvent::Open {
      workspace_id: "w1".to_string(),
      object_id: "2".to_string(),
      collab_type: CollabType::Unknown,
      doc_state: vec![],
    },
    recv_event
  );
}

fn mock_event(object_id: String) -> CollabControlEvent {
  CollabControlEvent::Open {
    workspace_id: "w1".to_string(),
    object_id,
    collab_type: CollabType::Unknown,
    doc_state: vec![],
  }
}
