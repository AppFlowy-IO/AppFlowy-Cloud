use bytes::Bytes;
use collab::core::collab::default_client_id;
use collab_document::document_data::default_document_collab_data;
use collab_entity::CollabType;
use database::collab::insert_into_af_collab;
use database::index::{get_collab_embedding_fragment, upsert_collab_embeddings, Fragment};
use database_entity::dto::{AFCollabEmbeddedChunk, CollabParams};
use lazy_static::lazy_static;
use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};
use snowflake::Snowflake;
use sqlx::PgPool;
use tokio::sync::RwLock;
use uuid::Uuid;

pub async fn setup_db(pool: &PgPool) -> anyhow::Result<()> {
  // Have to manually create schema and tables managed by gotrue but referenced by our
  // migration scripts.
  sqlx::query(r#"create schema auth"#).execute(pool).await?;
  sqlx::query(
    r#"
      CREATE TABLE auth.users(
        id uuid NOT NULL UNIQUE,
        deleted_at timestamptz null,
        CONSTRAINT users_pkey PRIMARY KEY (id)
      )
    "#,
  )
  .execute(pool)
  .await?;

  sqlx::migrate!("./migrations")
    .set_ignore_missing(true)
    .run(pool)
    .await
    .unwrap();
  Ok(())
}

pub async fn insert_auth_user(pool: &PgPool, user_uuid: Uuid) -> anyhow::Result<()> {
  sqlx::query(
    r#"
      INSERT INTO auth.users (id)
      VALUES ($1)
    "#,
  )
  .bind(user_uuid)
  .execute(pool)
  .await?;
  Ok(())
}

lazy_static! {
  pub static ref ID_GEN: RwLock<Snowflake> = RwLock::new(Snowflake::new(1));
}

pub async fn create_test_user(
  pool: &PgPool,
  user_uuid: Uuid,
  email: &str,
  name: &str,
) -> anyhow::Result<TestUser> {
  insert_auth_user(pool, user_uuid).await.unwrap();
  let uid = ID_GEN.write().await.next_id();
  let workspace_id = database::user::create_user(pool, uid, &user_uuid, email, name)
    .await
    .unwrap();

  Ok(TestUser { uid, workspace_id })
}

pub async fn create_test_collab_document(
  pg_pool: &PgPool,
  uid: &i64,
  workspace_id: &Uuid,
  doc_id: &Uuid,
) {
  let document = default_document_collab_data(&doc_id.to_string(), default_client_id()).unwrap();
  let params = CollabParams {
    object_id: *doc_id,
    encoded_collab_v1: Bytes::from(document.encode_to_bytes().unwrap()),
    collab_type: CollabType::Document,
    updated_at: None,
  };

  let mut txn = pg_pool.begin().await.unwrap();
  insert_into_af_collab(&mut txn, uid, workspace_id, &params)
    .await
    .unwrap();
  txn.commit().await.unwrap();
}

pub async fn upsert_test_chunks(
  pg: &PgPool,
  workspace_id: &Uuid,
  doc_id: &Uuid,
  chunks: Vec<AFCollabEmbeddedChunk>,
) {
  let mut txn = pg.begin().await.unwrap();
  upsert_collab_embeddings(&mut txn, workspace_id, doc_id, 0, chunks.clone())
    .await
    .unwrap();
  txn.commit().await.unwrap();
}

pub async fn select_all_fragments(pg: &PgPool, object_id: &Uuid) -> Vec<Fragment> {
  get_collab_embedding_fragment(pg, object_id).await.unwrap()
}

#[derive(Clone)]
pub struct TestUser {
  pub uid: i64,
  pub workspace_id: Uuid,
}

pub fn generate_random_bytes(size: usize) -> Vec<u8> {
  let s: String = thread_rng()
    .sample_iter(&Alphanumeric)
    .take(size)
    .map(char::from)
    .collect();
  s.into_bytes()
}
