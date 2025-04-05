use client_api::entity::{QueryCollab, QueryCollabParams, UpdateCollabWebParams};
use client_api_test::{
  assert_client_collab_within_secs, assert_server_collab, generate_unique_registered_user,
  TestClient,
};
use collab_entity::CollabType;
use serde_json::json;
use yrs::{updates::decoder::Decode, Map, ReadTxn, StateVector, Transact};

#[tokio::test]
async fn web_and_native_app_edit_same_collab_test() {
  let collab_type = CollabType::Unknown;
  let registered_user = generate_unique_registered_user().await;
  let mut app_client = TestClient::user_with_new_device(registered_user.clone()).await;
  let workspace_id = app_client.workspace_id().await;
  let object_id = app_client
    .create_and_edit_collab(workspace_id, collab_type)
    .await;

  // client 1 edit the collab
  app_client
    .insert_into(&object_id, "name", "workspace1")
    .await;
  app_client
    .wait_object_sync_complete(&object_id)
    .await
    .unwrap();
  assert_server_collab(
    workspace_id,
    &mut app_client.api_client,
    object_id,
    &collab_type,
    30,
    json!({
      "name": "workspace1"
    }),
  )
  .await
  .unwrap();
  // AppFlowy Web does not actually rely on the Rust client, the below is just to emulate
  // the behaviour of the web frontend's javascript client.
  let web_client = TestClient::user_with_new_device(registered_user.clone()).await;
  let collab_doc_state = web_client
    .api_client
    .get_collab(QueryCollabParams {
      workspace_id,
      inner: QueryCollab {
        object_id,
        collab_type,
      },
    })
    .await
    .unwrap()
    .encode_collab
    .doc_state;
  let web_doc = yrs::Doc::new();
  let update = yrs::Update::decode_v1(&collab_doc_state).unwrap();
  web_doc.transact_mut().apply_update(update).unwrap();
  let doc_data = web_doc.transact().get_map("data").unwrap();
  {
    let mut txn = web_doc.transact_mut();
    doc_data.insert(&mut txn, "paragraph", "content");
  }
  web_client
    .api_client
    .update_web_collab(
      &workspace_id,
      &object_id,
      UpdateCollabWebParams {
        doc_state: web_doc
          .transact()
          .encode_state_as_update_v1(&StateVector::default()),
        collab_type,
      },
    )
    .await
    .unwrap();

  let expected_json = json!({
    "name": "workspace1",
    "paragraph": "content",
  });
  assert_server_collab(
    workspace_id,
    &mut app_client.api_client,
    object_id,
    &collab_type,
    30,
    expected_json.clone(),
  )
  .await
  .unwrap();

  assert_client_collab_within_secs(
    &mut app_client,
    &object_id,
    "paragraph",
    expected_json.clone(),
    30,
  )
  .await;
}
