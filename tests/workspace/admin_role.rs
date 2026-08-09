//! Integration tests for the cloud-side Admin role.
//!
//! See ADMIN_ROLE_SPEC.md §5 for the scenarios covered here. These tests do
//! not exercise the Flutter client — they hit the cloud HTTP API directly via
//! `client-api-test::TestClient`.
//!
//! Three invariants are asserted:
//!   1. The owner is the final authority — only Owner may promote/demote to
//!      Owner or Admin, or delete an Owner.
//!   2. A workspace must always have at least one Owner.
//!   3. An Admin can update Member <-> Admin, but cannot touch the Owner.

use app_error::ErrorCode;
use client_api_test::TestClient;
use database_entity::dto::AFRole;

/// Helper: build a workspace with two Owners and one Member from scratch.
async fn workspace_with_two_owners_one_member(
) -> (TestClient, TestClient, TestClient, uuid::Uuid) {
  let owner = TestClient::new_user_without_ws_conn().await;
  let second_owner = TestClient::new_user_without_ws_conn().await;
  let member = TestClient::new_user_without_ws_conn().await;
  let workspace_id = owner.workspace_id().await;

  owner
    .invite_and_accepted_workspace_member(&workspace_id, &second_owner, AFRole::Owner)
    .await
    .unwrap();
  second_owner
    .invite_and_accepted_workspace_member(&workspace_id, &member, AFRole::Member)
    .await
    .unwrap();

  (owner, second_owner, member, workspace_id)
}

#[tokio::test]
async fn owner_promotes_member_to_admin() {
  let (owner, _second_owner, member, workspace_id) = workspace_with_two_owners_one_member().await;

  owner
    .try_update_workspace_member(&workspace_id, &member, AFRole::Admin)
    .await
    .unwrap();

  let members = owner.get_workspace_members(&workspace_id).await;
  let member_email = member.email().await;
  let promoted = members
    .iter()
    .find(|m| m.email == member_email)
    .unwrap();
  assert_eq!(promoted.role, AFRole::Admin);
}

#[tokio::test]
async fn admin_promotes_member_to_admin() {
  let owner = TestClient::new_user_without_ws_conn().await;
  let admin = TestClient::new_user_without_ws_conn().await;
  let member = TestClient::new_user_without_ws_conn().await;
  let workspace_id = owner.workspace_id().await;

  owner
    .invite_and_accepted_workspace_member(&workspace_id, &admin, AFRole::Admin)
    .await
    .unwrap();
  admin
    .invite_and_accepted_workspace_member(&workspace_id, &member, AFRole::Member)
    .await
    .unwrap();

  admin
    .try_update_workspace_member(&workspace_id, &member, AFRole::Admin)
    .await
    .unwrap();

  let members = admin.get_workspace_members(&workspace_id).await;
  let member_email = member.email().await;
  let promoted = members
    .iter()
    .find(|m| m.email == member_email)
    .unwrap();
  assert_eq!(promoted.role, AFRole::Admin);
}

#[tokio::test]
async fn admin_cannot_promote_member_to_owner() {
  let owner = TestClient::new_user_without_ws_conn().await;
  let admin = TestClient::new_user_without_ws_conn().await;
  let member = TestClient::new_user_without_ws_conn().await;
  let workspace_id = owner.workspace_id().await;

  owner
    .invite_and_accepted_workspace_member(&workspace_id, &admin, AFRole::Admin)
    .await
    .unwrap();
  admin
    .invite_and_accepted_workspace_member(&workspace_id, &member, AFRole::Member)
    .await
    .unwrap();

  let error = admin
    .try_update_workspace_member(&workspace_id, &member, AFRole::Owner)
    .await
    .unwrap_err();
  assert_eq!(error.code, ErrorCode::NotEnoughPermissions);

  // Member must still be a Member after the rejected demotion attempt.
  let members = admin.get_workspace_members(&workspace_id).await;
  let member_email = member.email().await;
  let member_row = members
    .iter()
    .find(|m| m.email == member_email)
    .unwrap();
  assert_eq!(member_row.role, AFRole::Member);
}

#[tokio::test]
async fn admin_cannot_delete_owner() {
  let owner = TestClient::new_user_without_ws_conn().await;
  let admin = TestClient::new_user_without_ws_conn().await;
  let workspace_id = owner.workspace_id().await;

  owner
    .invite_and_accepted_workspace_member(&workspace_id, &admin, AFRole::Admin)
    .await
    .unwrap();

  // Even if the Admin tries to remove the Owner, the cloud rejects it. The
  // owner_uid in af_workspace triggers NotEnoughPermissions from the SQL
  // check, which surfaces the same code the Admin-only guard would.
  let error = admin
    .try_remove_workspace_member(&workspace_id, &owner)
    .await
    .unwrap_err();
  assert_eq!(error.code, ErrorCode::NotEnoughPermissions);

  let members = admin.get_workspace_members(&workspace_id).await;
  assert_eq!(members.len(), 2);
}

#[tokio::test]
async fn owner_demotes_admin_to_member() {
  let owner = TestClient::new_user_without_ws_conn().await;
  let admin = TestClient::new_user_without_ws_conn().await;
  let workspace_id = owner.workspace_id().await;

  owner
    .invite_and_accepted_workspace_member(&workspace_id, &admin, AFRole::Admin)
    .await
    .unwrap();

  owner
    .try_update_workspace_member(&workspace_id, &admin, AFRole::Member)
    .await
    .unwrap();

  let members = owner.get_workspace_members(&workspace_id).await;
  let admin_email = admin.email().await;
  let admin_row = members
    .iter()
    .find(|m| m.email == admin_email)
    .unwrap();
  assert_eq!(admin_row.role, AFRole::Member);
}

#[tokio::test]
async fn owner_cannot_demote_the_only_owner() {
  let owner = TestClient::new_user_without_ws_conn().await;
  let workspace_id = owner.workspace_id().await;

  // Workspace created by just `owner`; no second Owner exists, so demoting
  // the only Owner must be refused.
  let error = owner
    .try_update_workspace_member(&workspace_id, &owner, AFRole::Member)
    .await
    .unwrap_err();
  assert_eq!(error.code, ErrorCode::NotEnoughPermissions);

  let members = owner.get_workspace_members(&workspace_id).await;
  assert_eq!(members.len(), 1);
  assert_eq!(members[0].role, AFRole::Owner);
}

#[tokio::test]
async fn admin_lists_workspace_members_succeeds() {
  let owner = TestClient::new_user_without_ws_conn().await;
  let admin = TestClient::new_user_without_ws_conn().await;
  let workspace_id = owner.workspace_id().await;

  owner
    .invite_and_accepted_workspace_member(&workspace_id, &admin, AFRole::Admin)
    .await
    .unwrap();

  // The explicit "Admin can list" test mirrors the existing
  // enforce_role_weak(..., Guest) coarse gate: Admin satisfies Guest-already.
  let members = admin.get_workspace_members(&workspace_id).await;
  let owner_email = owner.email().await;
  let admin_email = admin.email().await;
  assert!(members.iter().any(|m| m.email == owner_email));
  assert!(members.iter().any(|m| m.email == admin_email));
  assert_eq!(members.len(), 2);
}

#[tokio::test]
async fn two_owners_coexist_one_demotes_the_other() {
  let owner = TestClient::new_user_without_ws_conn().await;
  let second_owner = TestClient::new_user_without_ws_conn().await;
  let workspace_id = owner.workspace_id().await;

  owner
    .invite_and_accepted_workspace_member(&workspace_id, &second_owner, AFRole::Owner)
    .await
    .unwrap();

  // second_owner demotes owner -> Member. Two Owners exist, so the
  // last-Owner guard permits one of them to be demoted.
  second_owner
    .try_update_workspace_member(&workspace_id, &owner, AFRole::Member)
    .await
    .unwrap();

  let members = second_owner.get_workspace_members(&workspace_id).await;
  let owner_email = owner.email().await;
  let second_owner_email = second_owner.email().await;
  let owner_row = members
    .iter()
    .find(|m| m.email == owner_email)
    .unwrap();
  assert_eq!(owner_row.role, AFRole::Member);
  let second_owner_row = members
    .iter()
    .find(|m| m.email == second_owner_email)
    .unwrap();
  assert_eq!(second_owner_row.role, AFRole::Owner);
}
