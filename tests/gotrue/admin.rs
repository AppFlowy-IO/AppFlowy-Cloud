use client_api_test::*;
use gotrue::{
  api::Client,
  grant::{Grant, PasswordGrant},
  params::{AdminDeleteUserParams, AdminUserParams, GenerateLinkParams},
};

#[tokio::test]
async fn admin_user_create_list_edit_delete() {
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

  // list users
  let users = gotrue_client
    .admin_list_user(&admin_token.access_token, None)
    .await
    .unwrap()
    .users;

  // should be able to find user that was just created
  let new_user = users.iter().find(|u| u.email == user_email).unwrap();

  // change password for user
  let new_password = "Hello456!";
  let _ = gotrue_client
    .admin_update_user(
      &admin_token.access_token,
      new_user.id.as_str(),
      &AdminUserParams {
        email: user_email.clone(),
        password: Some(new_password.to_owned()),
        ..Default::default()
      },
    )
    .await
    .unwrap();
  assert_eq!(user.email, user_email);
  assert!(user.email_confirmed_at.is_some());

  // login user with new password
  let _ = gotrue_client
    .token(&Grant::Password(PasswordGrant {
      email: user_email.clone(),
      password: new_password.to_string(),
    }))
    .await
    .unwrap();

  // delete user that was just created
  gotrue_client
    .admin_delete_user(
      &admin_token.access_token,
      &new_user.id,
      &AdminDeleteUserParams {
        should_soft_delete: true,
      },
    )
    .await
    .unwrap();

  let users = gotrue_client
    .admin_list_user(&admin_token.access_token, None)
    .await
    .unwrap()
    .users;

  // user list should not contain the new user added
  // since it's deleted
  let found = users.iter().any(|u| u.email == user_email);
  assert!(!found);
}

#[tokio::test]
async fn admin_generate_link_and_user_sign_in_and_invite() {
  // admin generate link for new user
  let new_user_sign_in_link = {
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

    // create link
    let admin_user_params: GenerateLinkParams = GenerateLinkParams {
      email: user_email.clone(),
      ..Default::default()
    };
    let link_resp = gotrue_client
      .admin_generate_link(&admin_token.access_token, &admin_user_params)
      .await
      .unwrap();

    assert_eq!(link_resp.email, user_email);
    link_resp.action_link
  };

  // new user sign in with link,
  // invite another user through magic link
  {
    let client = localhost_client();
    let appflowy_sign_in_url = client
      .extract_sign_in_url(&new_user_sign_in_link)
      .await
      .unwrap();

    let is_new = client
      .sign_in_with_url(&appflowy_sign_in_url)
      .await
      .unwrap();
    assert!(is_new);

    let workspaces = client.get_workspaces().await.unwrap();
    assert_eq!(workspaces.0.len(), 1);

    let friend_email = generate_unique_email();
    client.invite(&friend_email).await.unwrap();
  }
}
