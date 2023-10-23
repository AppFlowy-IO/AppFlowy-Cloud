use client_api::Client;

mod member_crud;
mod storage_test;

mod edit_permission;
mod multi_devices_edit;
mod single_device_edit;
mod workspace_collab;

pub(crate) async fn workspace_id_from_client(c: &Client) -> String {
  c.get_workspaces()
    .await
    .unwrap()
    .0
    .first()
    .unwrap()
    .workspace_id
    .to_string()
}
