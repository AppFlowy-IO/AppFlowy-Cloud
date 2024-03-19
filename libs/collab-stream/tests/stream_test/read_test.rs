use crate::stream_test::test_util::stream_client;
use collab_stream::model::Message;

#[tokio::test]
async fn read_message_from_stream_test() {
  let client = stream_client().await;

  let msg = Message {
    uid: 1,
    raw_data: vec![1, 2, 3],
  };
  let mut stream_1 = client.stream("w1", "o1").await;
  let mut stream_2 = client.stream("w1", "o1").await;
  stream_1.insert_message(msg).await;

  let msg = stream_2.wait_one_update(None).await.unwrap();
}
