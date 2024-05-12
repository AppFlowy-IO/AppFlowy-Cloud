use client_api_test::TestClient;

#[tokio::test]
async fn workspace_usage_put_blob_test() {
  let client = TestClient::new_user_without_ws_conn().await;
  let mime = mime::TEXT_PLAIN_UTF_8;
  let file_id_1 = uuid::Uuid::new_v4().to_string();
  let file_id_2 = uuid::Uuid::new_v4().to_string();
  client.upload_blob(&file_id_1, "123", &mime).await;
  client.upload_blob(&file_id_2, "456", &mime).await;

  let usage = client.get_workspace_usage().await;
  assert_eq!(usage.consumed_capacity, 6);

  // after the test, delete the files
  client.delete_file(&file_id_1).await;
  client.delete_file(&file_id_2).await;
}

#[tokio::test]
async fn workspace_usage_put_and_then_delete_blob_test() {
  let client = TestClient::new_user_without_ws_conn().await;
  let mime = mime::TEXT_PLAIN_UTF_8;
  let file_id_1 = uuid::Uuid::new_v4().to_string();
  let file_id_2 = uuid::Uuid::new_v4().to_string();
  client.upload_blob(&file_id_1, "123", &mime).await;
  client.upload_blob(&file_id_2, "456", &mime).await;

  client.delete_file(&file_id_1).await;
  let usage = client.get_workspace_usage().await;
  assert_eq!(usage.consumed_capacity, 3);

  client.delete_file(&file_id_2).await;
  let usage = client.get_workspace_usage().await;
  assert_eq!(usage.consumed_capacity, 0);
}
