use client_api::extract_sign_in_url;
use gotrue::{
  api::Client,
  grant::{Grant, PasswordGrant},
  params::{AdminUserParams, GenerateLinkParams},
};

use crate::{
  localhost_client,
  user::utils::{generate_unique_email, ADMIN_USER},
  LOCALHOST_GOTRUE,
};

#[tokio::test]
async fn admin_user_create_and_list() {
  let http_client = reqwest::Client::new();
  let gotrue_client = Client::new(http_client, "http://localhost:9998");
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

  let users = gotrue_client
    .admin_list_user(&admin_token.access_token)
    .await
    .unwrap();
  assert!(users.users.len() > 2);
}

#[tokio::test]
async fn admin_generate_link_and_user_sign_in() {
  let http_client = reqwest::Client::new();
  let gotrue_client = Client::new(http_client, LOCALHOST_GOTRUE);
  let admin_token = gotrue_client
    .token(&Grant::Password(PasswordGrant {
      email: ADMIN_USER.email.clone(),
      password: ADMIN_USER.password.clone(),
    }))
    .await
    .unwrap();

  // new user params
  let user_email = generate_unique_email();

  // create link
  let admin_user_params: GenerateLinkParams = GenerateLinkParams {
    email: user_email.clone(),
    ..Default::default()
  };
  let link_resp = gotrue_client
    .generate_link(&admin_token.access_token, &admin_user_params)
    .await
    .unwrap();

  assert_eq!(link_resp.email, user_email);

  // visit action link
  let action_link = link_resp.action_link;
  let reqwest_client = reqwest::Client::new();
  let resp = reqwest_client.get(action_link).send().await.unwrap();
  let resp_text = resp.text().await.unwrap();
  let appflowy_sign_in_url = extract_sign_in_url(&resp_text).unwrap();

  let client = localhost_client();
  let is_new = client
    .sign_in_with_url(&appflowy_sign_in_url)
    .await
    .unwrap();
  assert!(is_new);

  let workspaces = client.workspaces().await.unwrap();
  assert_eq!(workspaces.len(), 1);
}
