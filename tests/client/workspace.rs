use crate::{user_1_signed_in, user_2_signed_in};

#[tokio::test]
async fn workspace_members() {
  let mut c1 = user_1_signed_in().await;
  let c2 = user_2_signed_in().await;

  let binding = c1.workspaces().await.unwrap();
  let ws1 = binding.first().unwrap();

  c1.add_workspace_members(
    ws1.workspace_id,
    [c2.token().unwrap().user.email.to_owned()].to_vec(),
  )
  .await
  .unwrap();

  // TODO: check

  c1.add_workspace_members(
    ws1.workspace_id,
    [c2.token().unwrap().user.email.to_owned()].to_vec(),
  )
  .await
  .unwrap();

  c1.remove_workspace_members(
    ws1.workspace_id,
    [c2.token().unwrap().user.email.to_owned()].to_vec(),
  )
  .await
  .unwrap();

  c1.remove_workspace_members(
    ws1.workspace_id,
    [c2.token().unwrap().user.email.to_owned()].to_vec(),
  )
  .await
  .unwrap();
}
