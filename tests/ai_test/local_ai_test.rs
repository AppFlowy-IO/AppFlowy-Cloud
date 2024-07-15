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
  {
    assert!(!config.models.is_empty());
    assert!(!config.models[0].embedding_model.download_url.is_empty());
    assert!(config.models[0].embedding_model.file_size > 10);
    assert!(!config.models[0].chat_model.download_url.is_empty());
    assert!(config.models[0].chat_model.file_size > 10);

    assert!(!config.plugin.version.is_empty());
    assert!(!config.plugin.url.is_empty());
    println!("config: {:?}", config);
  }
}
