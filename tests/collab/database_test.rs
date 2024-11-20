use client_api_test::TestClient;
use collab_database::database::Database;
use collab_database::rows::CreateRowParams;
use collab_entity::CollabType;
use uuid::Uuid;

#[tokio::test]
async fn create_edit_database_rows() {
  let database_id = Uuid::new_v4().to_string();
  let collab_type = CollabType::Document;

  let mut client = TestClient::new_user().await;
  let workspace_id = client.workspace_id().await;
  client
    .open_collab(&workspace_id, &database_id, collab_type.clone())
    .await;

  let mut database = client.get_database(&workspace_id, &database_id).await;
  const ROW_COUNT: usize = 5;
  let mut rows = Vec::with_capacity(ROW_COUNT);
  for _ in 0..ROW_COUNT {
    let row_id = Uuid::new_v4().to_string();
    rows.push(row_id.clone());
    let params = CreateRowParams::new(row_id, database_id.clone());
    database.create_row(params).await.unwrap();
  }

  for row_id in rows.iter() {
    client.wait_object_sync_complete(&row_id).await.unwrap();
  }
}
