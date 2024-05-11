use client_api_test::TestClient;
use collab_entity::CollabType;
use database_entity::dto::{AFAccessLevel, AFRole};
use std::time::Duration;
use tokio::time::sleep;

#[tokio::test]
async fn viewing_document_editing_users_test() {
  let collab_type = CollabType::Unknown;
  let mut owner = TestClient::new_user().await;
  let mut guest = TestClient::new_user().await;

  let workspace_id = owner.workspace_id().await;
  owner
    .invite_and_accepted_workspace_member(&workspace_id, &guest, AFRole::Member)
    .await
    .unwrap();

  let object_id = owner
    .create_and_edit_collab(&workspace_id, collab_type.clone())
    .await;

  let owner_uid = owner.uid().await;
  let clients = owner.get_connect_users(&object_id).await;
  assert_eq!(clients.len(), 1);
  assert_eq!(clients[0], owner_uid);

  owner
    .add_collab_member(
      &workspace_id,
      &object_id,
      &guest,
      AFAccessLevel::ReadAndWrite,
    )
    .await;
  guest
    .open_collab(&workspace_id, &object_id, collab_type)
    .await;
  guest.wait_object_sync_complete(&object_id).await.unwrap();

  // after guest open the collab, it will emit an awareness that contains the user id of guest.
  // This awareness will be sent to the server. Server will broadcast the awareness to all the clients
  // that are connected to the collab.
  let guest_uid = guest.uid().await;
  let mut clients: Vec<i64> = owner.get_connect_users(&object_id).await;
  clients.sort();

  let mut expected_clients = [owner_uid, guest_uid];
  expected_clients.sort();

  assert_eq!(clients.len(), 2);
  assert_eq!(clients, expected_clients);
  // simulate the guest close the collab
  guest.clean_awareness_state(&object_id);
  // sleep 5 second to make sure the awareness observe callback is called
  sleep(Duration::from_secs(5)).await;
  guest.wait_object_sync_complete(&object_id).await.unwrap();
  let clients = owner.get_connect_users(&object_id).await;
  assert_eq!(clients.len(), 1);
  assert_eq!(clients[0], owner_uid);

  // simulate the guest open the collab again
  guest.emit_awareness_state(&object_id);
  // sleep 2 second to make sure the awareness observe callback is called
  sleep(Duration::from_secs(2)).await;
  guest.wait_object_sync_complete(&object_id).await.unwrap();

  assert_num_connected_client_within_secs(&owner, &object_id, 2, 30).await;
}

async fn assert_num_connected_client_within_secs(
  client: &TestClient,
  object_id: &str,
  expected: usize,
  secs: u64,
) {
  let mut retry_count = 0;
  loop {
    tokio::select! {
       _ = tokio::time::sleep(Duration::from_secs(secs)) => {
         panic!("timeout");
       },
       clients = client.get_connect_users(object_id) => {
        retry_count += 1;
        if retry_count > 30 {
          assert_eq!(clients.len(), expected);
          break;
        }
        if clients.len() == expected {
            break;
        }
        tokio::time::sleep(Duration::from_millis(1000)).await;
      }
    }
  }
}
