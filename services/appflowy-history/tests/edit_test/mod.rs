use crate::edit_test::mock::mock_event;
use crate::util::{redis_stream, setup_db};
use appflowy_history::core::manager::OpenCollabManager;

use collab_stream::client::CONTROL_STREAM_KEY;
use sqlx::PgPool;
use std::time::Duration;
use tokio::time::sleep;

mod mock;

#[sqlx::test(migrations = false)]
async fn test(pool: PgPool) {
  setup_db(&pool).await.unwrap();
  let redis_stream = redis_stream().await;
  let _manager = OpenCollabManager::new(redis_stream.clone(), pool).await;

  let workspace_id = uuid::Uuid::new_v4().to_string();
  let object_id = uuid::Uuid::new_v4().to_string();
  let mock = mock_event(&workspace_id, &object_id).await;

  let mut control_group = redis_stream
    .collab_control_stream(CONTROL_STREAM_KEY, "appflowy_cloud")
    .await
    .unwrap();

  // apply open event
  control_group
    .insert_message(mock.open_event.clone())
    .await
    .unwrap();

  let mut update_group = redis_stream
    .collab_update_stream(&workspace_id, &object_id, "appflowy_cloud")
    .await
    .unwrap();

  // apply updates
  for update_event in &mock.update_events {
    update_group
      .insert_message(update_event.clone())
      .await
      .unwrap();
  }

  // apply close event
  control_group
    .insert_message(mock.close_event.clone())
    .await
    .unwrap();

  sleep(Duration::from_secs(10)).await;
}
