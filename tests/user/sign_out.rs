use client_api_test::*;

#[tokio::test]
async fn sign_out_but_not_sign_in() {
  let c = localhost_client();
  let res = c.sign_out().await;
  assert!(res.is_err());
}

#[tokio::test]
async fn sign_out_after_sign_in() {
  let (c, user) = generate_unique_registered_user_client().await;
  c.sign_in_password(&user.email, &user.password)
    .await
    .unwrap();
  c.sign_out().await.unwrap();
}
