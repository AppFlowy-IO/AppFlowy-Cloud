use client_api_test::generate_unique_registered_user_client;

#[tokio::test]
async fn test_get_server_info() {
  let (c, _) = generate_unique_registered_user_client().await;
  c.get_server_info()
    .await
    .expect("Failed to get server info");
}
