use client_api_test::TestClient;

#[tokio::test]
async fn get_local_ai_config_test() {
  let test_client = TestClient::new_user().await;
  let workspace_id = test_client.workspace_id().await;
  let config = test_client
    .api_client
    .get_local_ai_config(&workspace_id, "macos")
    .await
    .unwrap();
  assert!(config.llm_config.embedding_models.len() > 0);
  assert!(config.llm_config.llm_models.len() > 0);
  assert!(config.package.url.len() > 0);
}
