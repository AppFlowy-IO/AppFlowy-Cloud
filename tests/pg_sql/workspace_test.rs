use crate::pg_sql::util::{setup_db, test_create_user};
use sqlx::PgPool;

#[sqlx::test(migrations = false)]
async fn basic_test(pool: PgPool) -> sqlx::Result<()> {
  setup_db(&pool).await.unwrap();

  let user_uuid = uuid::Uuid::new_v4();
  let name = user_uuid.to_string();
  let email = format!("{}@appflowy.io", name);

  test_create_user(&pool, user_uuid, &email, &name)
    .await
    .unwrap();

  Ok(())
}
