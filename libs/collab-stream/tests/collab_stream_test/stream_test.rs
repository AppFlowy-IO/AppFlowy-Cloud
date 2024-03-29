use crate::collab_stream_test::test_util::{random_i64, stream_client};
use collab_stream::model::Message;

#[tokio::test]
async fn read_single_message_test() {
  let oid = format!("o{}", random_i64());
  let client_2 = stream_client().await;
  let mut stream_2 = client_2.stream("w1", &oid).await;

  let (tx, mut rx) = tokio::sync::mpsc::channel(1);
  tokio::spawn(async move {
    let msg = stream_2.next().await.unwrap();
    tx.send(msg).await.unwrap();
  });

  let msg = Message {
    uid: 3,
    raw_data: vec![1, 2, 3],
  };

  {
    let client_1 = stream_client().await;
    let mut stream_1 = client_1.stream("w1", &oid).await;
    stream_1.insert_message(msg).await.unwrap();
  }

  let msg = rx.recv().await.unwrap().unwrap();
  assert_eq!(msg.raw_data, vec![1, 2, 3]);
}

#[tokio::test]
async fn read_multiple_messages_test() {
  let oid = format!("o{}", random_i64());
  let client_2 = stream_client().await;
  let mut stream_2 = client_2.stream("w1", &oid).await;
  stream_2.clear().await.unwrap();

  {
    let client_1 = stream_client().await;
    let mut stream_1 = client_1.stream("w1", &oid).await;
    let messages = vec![
      Message {
        uid: 1001,
        raw_data: vec![1, 2, 3],
      },
      Message {
        uid: 1002,
        raw_data: vec![4, 5, 6],
      },
      Message {
        uid: 1003,
        raw_data: vec![7, 8, 9],
      },
    ];
    stream_1.insert_messages(messages).await.unwrap();
  }

  let msg = stream_2.read_all_message().await.unwrap();
  assert_eq!(msg.len(), 3);
  assert_eq!(msg[0].raw_data, vec![1, 2, 3]);
  assert_eq!(msg[0].uid, 1001);
  assert_eq!(msg[1].raw_data, vec![4, 5, 6]);
  assert_eq!(msg[1].uid, 1002);
  assert_eq!(msg[2].raw_data, vec![7, 8, 9]);
  assert_eq!(msg[2].uid, 1003);
}
