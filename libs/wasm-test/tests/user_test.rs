use client_api_test_util::{generate_unique_email, localhost_client};
use wasm_bindgen_test::wasm_bindgen_test;

#[wasm_bindgen_test]
async fn wasm_sign_up_success() {
  let email = generate_unique_email();
  let password = "Hello!123#";
  let c = localhost_client();
  c.sign_up(&email, password).await.unwrap();
}
