use crate::util::test_client::TestClient;

const TEN_MB: usize = 10 * 1024 * 1024;
const FIVE_MB: usize = 5 * 1024 * 1024;

#[tokio::test]
async fn workspace_usage_put_blob_test() {
  let client = TestClient::new_user_without_ws_conn().await;
  let mime = mime::TEXT_PLAIN_UTF_8;
  let file_1 = client.upload_file_with_size("10 mb", &mime, TEN_MB).await;
  let file_2 = client.upload_file_with_size("5 mb", &mime, FIVE_MB).await;

  let usage = client.get_workspace_usage().await;
  assert_eq!(usage.consumed_capacity, (TEN_MB + FIVE_MB) as u64);

  client.delete_file(&file_1).await;
  client.delete_file(&file_2).await;
}

#[tokio::test]
async fn workspace_usage_put_and_then_delete_blob_test() {
  let client = TestClient::new_user_without_ws_conn().await;
  let mime = mime::TEXT_PLAIN_UTF_8;
  let file_1 = client.upload_file_with_size("10 mb", &mime, TEN_MB).await;
  let file_2 = client.upload_file_with_size("5 mb", &mime, FIVE_MB).await;

  client.delete_file(&file_1).await;
  let usage = client.get_workspace_usage().await;
  assert_eq!(usage.consumed_capacity, (FIVE_MB) as u64);

  client.delete_file(&file_2).await;
  assert_eq!(usage.consumed_capacity, 0);
}
