use client_api_test::{generate_unique_email, localhost_client, TestClient};
use wasm_bindgen_test::wasm_bindgen_test;

#[wasm_bindgen_test]
async fn wasm_sign_up_success() {
  let email = generate_unique_email();
  let password = "Hello!123#";
  let c = localhost_client();
  c.sign_up(&email, password).await.unwrap();
}

#[wasm_bindgen_test]
async fn wasm_sign_in_success() {
  let test_client = TestClient::new_user().await;
  let user = test_client.user;

  let res = test_client
    .api_client
    .sign_in_password(user.email.as_str(), user.password.as_str())
    .await;

  assert!(res.is_ok());

  let val = res.unwrap();

  assert!(val);
}

#[wasm_bindgen_test]
async fn wasm_logout_success() {
  let test_client = TestClient::new_user().await;
  let user = test_client.user;

  test_client
    .api_client
    .sign_in_password(user.email.as_str(), user.password.as_str())
    .await
    .unwrap();
  let res = test_client.api_client.sign_out().await;

  assert!(res.is_ok());
}
