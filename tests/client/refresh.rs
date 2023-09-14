use crate::{
  client::utils::{REGISTERED_EMAIL, REGISTERED_PASSWORD},
  client_api_client,
};

#[tokio::test]
async fn refresh_success() {
  let email = &REGISTERED_EMAIL;
  let password = &REGISTERED_PASSWORD;
  let mut c = client_api_client();
  c.sign_in_password(&email, password).await.unwrap();
  c.refresh().await.unwrap();
}
