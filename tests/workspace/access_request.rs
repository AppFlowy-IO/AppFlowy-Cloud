use app_error::ErrorCode;
use client_api::entity::CreateAccessRequestParams;
use client_api_test::generate_unique_registered_user_client;
use shared_entity::dto::workspace_dto::ViewLayout;

#[tokio::test]
async fn access_request_test() {
  let (owner_client, _) = generate_unique_registered_user_client().await;
  let workspaces = owner_client.get_workspaces().await.unwrap();
  let workspace_id = workspaces[0].workspace_id;
  let folder_view = owner_client
    .get_workspace_folder(&workspace_id, Some(2), None)
    .await
    .unwrap();
  let view_id = folder_view
    .children
    .into_iter()
    .find(|v| v.name == "General")
    .unwrap()
    .children
    .iter()
    .find(|v| v.name == "To-dos")
    .unwrap()
    .view_id
    .clone();
  let data = CreateAccessRequestParams {
    workspace_id,
    view_id,
  };
  let (requester_client, requester) = generate_unique_registered_user_client().await;
  let access_request = requester_client
    .create_access_request(data.clone())
    .await
    .unwrap();
  let resp = requester_client.create_access_request(data).await;
  assert!(resp.is_err());
  assert_eq!(
    resp.unwrap_err().code,
    ErrorCode::AccessRequestAlreadyExists
  );
  // Only workspace owner should be allowed to view access requests
  let resp = requester_client
    .get_access_request(access_request.request_id)
    .await;
  assert!(resp.is_err());
  assert_eq!(resp.unwrap_err().code, ErrorCode::NotEnoughPermissions);

  let access_request_id = access_request.request_id;
  let access_request_to_be_approved = owner_client
    .get_access_request(access_request_id)
    .await
    .unwrap();
  assert_eq!(
    access_request_to_be_approved.requester.email,
    requester.email
  );
  assert_eq!(
    access_request_to_be_approved.view.view_id,
    view_id.to_string()
  );
  assert_eq!(access_request_to_be_approved.view.layout, ViewLayout::Board);
  assert_eq!(
    access_request_to_be_approved.workspace.workspace_id,
    workspace_id
  );
  assert_eq!(
    access_request_to_be_approved.workspace.member_count,
    Some(1)
  );
  owner_client
    .approve_access_request(access_request_id)
    .await
    .unwrap();
  let workspace_members = owner_client
    .get_workspace_members(&workspace_id)
    .await
    .unwrap();
  assert!(workspace_members.iter().any(|m| m.email == requester.email));
}
