use client_api::entity::WorkspaceMemberProfile;
use client_api_test::generate_unique_registered_user_client;

#[tokio::test]
async fn workspace_mentionable_persons_crud() {
  let (c, user) = generate_unique_registered_user_client().await;
  let workspaces = c.get_workspaces().await.unwrap();
  assert_eq!(workspaces.len(), 1);
  let workspace_id = workspaces[0].workspace_id;
  c.update_workspace_member_profile(
    &workspace_id,
    &WorkspaceMemberProfile {
      name: "name override".to_string(),
      avatar_url: Some("avatar url override".to_string()),
      cover_image_url: Some("cover image url".to_string()),
      description: Some("description override".to_string()),
    },
  )
  .await
  .unwrap();

  let mentionable_persons = c
    .get_workspace_mentionable_persons(&workspace_id)
    .await
    .unwrap()
    .persons;
  assert_eq!(mentionable_persons.len(), 1);
  assert_eq!(mentionable_persons[0].email, user.email);
  assert_eq!(mentionable_persons[0].name, "name override");
}
