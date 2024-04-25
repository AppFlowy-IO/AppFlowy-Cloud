use client_api_test_util::*;

#[tokio::test]
async fn get_user_default_workspace_test() {
  let email = generate_unique_email();
  let password = "Hello!123#";
  let c = localhost_client();
  c.sign_up(&email, password).await.unwrap();
  let test_client = TestClient::new_user().await;
  let folder = test_client.get_user_folder().await;

  let views = folder.get_views_belong_to(&test_client.workspace_id().await);
  assert_eq!(views.len(), 1);
  assert_eq!(views[0].name, "Getting started");
}
