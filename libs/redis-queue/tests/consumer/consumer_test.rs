use crate::util::{redis_client, TestRedisQueueMessage};
use redis_queue::consumer::Consumer;
use redis_queue::producer::Producer;
use tokio::sync::mpsc::channel;

#[tokio::test]
async fn consumer_message_test() {
  let client = redis_client().await;
  let consumer = Consumer::new(
    "test".to_string(),
    "test_queue".to_string(),
    client.get_multiplexed_async_connection().await.unwrap(),
  );

  let (tx, mut rx) = channel(1);
  tokio::spawn(async move {
    while let Ok(mut next) = consumer.next::<TestRedisQueueMessage>().await {
      next.ack().await.unwrap();
      tx.send(next.into_message()).await.unwrap();
    }
  });

  let producer = Producer::new(
    "test_queue".to_string(),
    client.get_multiplexed_async_connection().await.unwrap(),
  );

  let msg = TestRedisQueueMessage { id: 1.to_string() };
  producer.push(msg.clone()).await.unwrap();

  let msg_from_queue = rx.recv().await.unwrap();
  assert_eq!(msg.id, msg_from_queue.id);
}
