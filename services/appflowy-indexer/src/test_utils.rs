use std::ops::DerefMut;
use std::sync::Arc;

use collab::core::collab::MutexCollab;
use collab_entity::CollabType;
use sqlx::PgPool;
use uuid::Uuid;
use yrs::Subscription;

use collab_stream::client::CollabRedisStream;
use collab_stream::model::CollabUpdateEvent;
use collab_stream::stream_group::StreamGroup;
use database::collab::insert_into_af_collab;
use database::user::create_user;
use database_entity::dto::CollabParams;

pub fn openai_client() -> openai_dive::v1::api::Client {
  let api_key = std::env::var("APPFLOWY_INDEXER_OPENAI_API_KEY").unwrap();
  openai_dive::v1::api::Client::new(api_key)
}

pub async fn db_pool() -> PgPool {
  let database_url = std::env::var("APPFLOWY_INDEXER_DATABASE_URL")
    .unwrap_or("postgres://postgres:password@localhost:5432/postgres".to_string());
  PgPool::connect(&database_url)
    .await
    .expect("failed to connect to database")
}

pub async fn setup_collab(db: &PgPool, uid: i64, object_id: Uuid, encoded_collab: Vec<u8>) -> Uuid {
  let mut tx = db.begin().await.unwrap();
  let user_uuid = Uuid::new_v4();
  sqlx::query("INSERT INTO auth.users(id) VALUES($1)")
    .bind(user_uuid)
    .execute(tx.deref_mut())
    .await
    .unwrap();
  let workspace_id = create_user(
    tx.deref_mut(),
    uid,
    &user_uuid,
    &format!("{user_uuid}@test.email"),
    &user_uuid.to_string(),
  )
  .await
  .unwrap();
  insert_into_af_collab(
    &mut tx,
    &uid,
    &workspace_id.to_string(),
    &CollabParams::new(object_id, CollabType::Document, encoded_collab),
  )
  .await
  .unwrap();
  tx.commit().await.unwrap();
  workspace_id
}

pub async fn redis_client() -> redis::Client {
  let redis_uri =
    std::env::var("APPFLOWY_INDEXER_REDIS_URL").unwrap_or("redis://localhost:6379".to_string());
  redis::Client::open(redis_uri).expect("failed to connect to redis")
}

pub async fn redis_stream() -> CollabRedisStream {
  let redis_client = redis_client().await;
  CollabRedisStream::new(redis_client)
    .await
    .expect("failed to create stream client")
}

pub fn collab_update_forwarder(collab: Arc<MutexCollab>, mut stream: StreamGroup) -> Subscription {
  let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
  tokio::spawn(async move {
    while let Some(data) = rx.recv().await {
      stream.insert_message(data).await.unwrap();
    }
  });
  let lock = collab.lock();
  lock
    .get_doc()
    .observe_update_v1(move |_, e| {
      let e = CollabUpdateEvent::UpdateV1 {
        encode_update: e.update.clone(),
      };
      tx.send(e).unwrap();
    })
    .unwrap()
}
