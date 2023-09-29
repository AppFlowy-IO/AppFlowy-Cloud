use collab_stream::{client::CollabStreamClient, collab_update::CollabUpdate};
use sqlx::types::Uuid;

#[tokio::test]
async fn write_and_read_stream() {
  let redis_client = redis::Client::open("redis://localhost:6380").unwrap();

  let collab_streamer = CollabStreamClient::new(redis_client).await.unwrap();

  let user_id = "5";
  let oid = Uuid::new_v4().to_string();
  let partition_key = 1;

  let mut doc_stream = collab_streamer.stream(&oid, partition_key).await;
  let _inserted_time = doc_stream
    .insert_one_update(CollabUpdate {
      uid: user_id.to_string(),
      raw_data: vec![1, 2, 3],
      ..Default::default()
    })
    .await
    .unwrap();

  let _inserted_time = doc_stream
    .insert_one_update(CollabUpdate {
      uid: user_id.to_string(),
      raw_data: vec![4, 5, 6],
      ..Default::default()
    })
    .await
    .unwrap();

  let _inserted_time = doc_stream
    .insert_one_update(CollabUpdate {
      uid: user_id.to_string(),
      raw_data: vec![7, 8, 9],
      ..Default::default()
    })
    .await
    .unwrap();

  let all_updates = doc_stream.read_all_updates().await.unwrap();
  let raw_data: Vec<u8> = all_updates.into_iter().flat_map(|u| u.raw_data).collect();
  assert_eq!(raw_data, vec![1, 2, 3, 4, 5, 6, 7, 8, 9])
}
