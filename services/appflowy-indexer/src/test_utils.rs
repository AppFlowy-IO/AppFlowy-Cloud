use collab::preclude::Collab;
use collab_stream::client::CollabRedisStream;
use collab_stream::model::CollabUpdateEvent;
use collab_stream::stream_group::StreamGroup;
use yrs::Subscription;

pub async fn redis_client() -> redis::Client {
  let redis_uri = "redis://localhost:6379";
  redis::Client::open(redis_uri).expect("failed to connect to redis")
}

pub async fn redis_stream() -> CollabRedisStream {
  let redis_client = redis_client().await;
  CollabRedisStream::new(redis_client)
    .await
    .expect("failed to create stream client")
}

pub fn collab_update_forwarder(collab: &mut Collab, mut stream: StreamGroup) -> Subscription {
  let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
  tokio::spawn(async move {
    while let Some(data) = rx.recv().await {
      println!("sending update to redis");
      stream.insert_message(data).await.unwrap();
    }
  });
  collab
    .get_doc()
    .observe_update_v1(move |_, e| {
      println!("Observed update");
      let e = CollabUpdateEvent::UpdateV1 {
        encode_update: e.update.clone(),
      };
      tx.send(e).unwrap();
    })
    .unwrap()
}
