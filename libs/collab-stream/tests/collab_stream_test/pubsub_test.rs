use crate::collab_stream_test::test_util::{pubsub_client, random_i64};

use collab_stream::pubsub::PubSubMessage;

use futures::StreamExt;
use std::time::Duration;
use tokio::time::sleep;

#[tokio::test]
async fn pubsub_test() {
  let oid = format!("o{}", random_i64());
  let client_1 = pubsub_client().await;
  let client_2 = pubsub_client().await;

  let mut publish = client_1.collab_pub().await;
  let send_msg = PubSubMessage {
    workspace_id: "1".to_string(),
    oid: oid.clone(),
  };

  let cloned_msg = send_msg.clone();
  tokio::spawn(async move {
    sleep(Duration::from_secs(1)).await;
    match publish.publish(cloned_msg).await {
      Ok(_) => {},
      Err(err) => {
        panic!("failed to publish message: {:?}", err);
      },
    }
  });

  let subscriber = client_2.collab_sub().await.unwrap();
  let mut pubsub = subscriber.subscribe().await.unwrap();
  let receive_msg = pubsub.next().await.unwrap().unwrap();

  assert_eq!(send_msg.workspace_id, receive_msg.workspace_id);
}
