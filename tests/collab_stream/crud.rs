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
    })
    .await
    .unwrap();

  let _inserted_time = doc_stream
    .insert_one_update(CollabUpdate {
      uid: user_id.to_string(),
      raw_data: vec![4, 5, 6],
    })
    .await
    .unwrap();

  let _inserted_time = doc_stream
    .insert_one_update(CollabUpdate {
      uid: user_id.to_string(),
      raw_data: vec![7, 8, 9],
    })
    .await
    .unwrap();

  let all_updates = doc_stream.read_all_updates().await.unwrap();
  let raw_data: Vec<u8> = all_updates.into_iter().flat_map(|u| u.raw_data).collect();
  assert_eq!(raw_data, vec![1, 2, 3, 4, 5, 6, 7, 8, 9])
}

#[tokio::test]
async fn wait_one_update_from_stream() {
  let user_id = "5";
  let oid = "1";
  let partition_key = 1;

  let handle1 = async {
    // Add a bit of delay so that handle2 can start waiting
    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

    let redis_client = redis::Client::open("redis://localhost:6380").unwrap();
    let collab_streamer = CollabStreamClient::new(redis_client).await.unwrap();
    let mut doc_stream = collab_streamer.stream(oid, partition_key).await;
    doc_stream
      .insert_one_update(CollabUpdate {
        uid: user_id.to_string(),
        raw_data: vec![1, 2, 3],
      })
      .await
  };

  let handle2 = async {
    let redis_client = redis::Client::open("redis://localhost:6380").unwrap();
    let collab_streamer = CollabStreamClient::new(redis_client).await.unwrap();
    let mut doc_stream = collab_streamer.stream(oid, partition_key).await;
    doc_stream.wait_one_update(None).await
  };

  let (_, got_from_waiting) = tokio::try_join!(handle1, handle2).unwrap();

  assert_eq!(got_from_waiting[0].raw_data, vec![1, 2, 3])
}
