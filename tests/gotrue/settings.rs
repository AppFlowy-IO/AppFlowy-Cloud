use client_api_test::{generate_unique_email, ADMIN_USER, LOCALHOST_GOTRUE};
use gotrue::{
  api::Client,
  grant::{Grant, PasswordGrant},
  params::AdminUserParams,
};

#[tokio::test]
async fn gotrue_settings() {
  let http_client = reqwest::Client::new();
  let gotrue_client = Client::new(http_client, &LOCALHOST_GOTRUE);
  gotrue_client.settings().await.unwrap();
}

#[tokio::test]
async fn admin_user_create() {
  let http_client = reqwest::Client::new();
  let gotrue_client = Client::new(http_client, &LOCALHOST_GOTRUE);
  let admin_token = gotrue_client
    .token(&Grant::Password(PasswordGrant {
      email: ADMIN_USER.email.clone(),
      password: ADMIN_USER.password.clone(),
    }))
    .await
    .unwrap();

  // new user params
  let user_email = generate_unique_email();
  let user_password = "Hello123!";

  // create user
  let admin_user_params: AdminUserParams = AdminUserParams {
    email: user_email.clone(),
    password: Some(user_password.to_string()),
    email_confirm: true,
    ..Default::default()
  };
  let user = gotrue_client
    .admin_add_user(&admin_token.access_token, &admin_user_params)
    .await
    .unwrap();
  assert_eq!(user.email, user_email);
  assert!(user.email_confirmed_at.is_some());

  // login user
  let user_token = gotrue_client
    .token(&Grant::Password(PasswordGrant {
      email: user_email.clone(),
      password: user_password.to_string(),
    }))
    .await
    .unwrap();
  assert!(user_token.user.email_confirmed_at.is_some());
}
