use crate::sql_test::util::{setup_db, test_create_user};
use sqlx::PgPool;

#[sqlx::test(migrations = false)]
async fn upsert_embeddings_test(pool: PgPool) {
  setup_db(&pool).await.unwrap();

  let user_uuid = uuid::Uuid::new_v4();
  let name = user_uuid.to_string();
  let email = format!("{}@appflowy.io", name);
  let user = test_create_user(&pool, user_uuid, &email, &name)
    .await
    .unwrap();
}
