use client_api_test::TestClient;

#[tokio::test]
async fn edit_workspace_without_permission() {
  let client = TestClient::new_user().await;
  let user_awareness = client.get_user_awareness().await;
  println!("user_awareness: {:?}", user_awareness.to_json());
}
