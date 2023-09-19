use crate::{user_1_signed_in, user_2_signed_in};

#[tokio::test]
async fn add_workspace_members() {
  let mut c1 = user_1_signed_in().await;
  let mut c2 = user_2_signed_in().await;
  let ws1 = c1.workspaces().await.unwrap().first().unwrap();
  let ws2 = c2.workspaces().await.unwrap().first().unwrap();


  todo!()
}
