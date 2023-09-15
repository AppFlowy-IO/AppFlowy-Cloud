use client_api::Client;

mod storage_test;

pub(crate) async fn workspace_id_from_client(c: &mut Client) -> String {
  c.workspaces()
    .await
    .unwrap()
    .first()
    .unwrap()
    .workspace_id
    .to_string()
}
