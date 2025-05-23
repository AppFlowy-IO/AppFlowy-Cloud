use app_error::ErrorCode;
use assert_json_diff::assert_json_include;
use collab::core::collab::default_client_id;
use collab::entity::EncodedCollab;
use collab_document::document_data::default_document_collab_data;
use collab_entity::CollabType;
use database_entity::dto::{
  CollabParams, CreateCollabData, CreateCollabParams, QueryCollab, QueryCollabParams,
  QueryCollabResult,
};

use reqwest::Method;
use serde::Serialize;
use serde_json::json;

use crate::collab::util::{empty_document_editor, generate_random_string, test_encode_collab_v1};
use client_api::process_response_data;
use client_api_test::TestClient;
use uuid::Uuid;

const WORKSPACE_ID: Uuid = Uuid::from_u128(70700);

#[tokio::test]
async fn get_collab_response_compatible_test() {
  let test_client = TestClient::new_user().await;
  let workspace_id = test_client.workspace_id().await;

  // after 0.3.22, we use [CollabResponse] instead of EncodedCollab as the response
  let collab_resp = test_client
    .get_collab(workspace_id, workspace_id, CollabType::Folder)
    .await
    .unwrap();
  assert_eq!(collab_resp.object_id, workspace_id);

  let json = serde_json::to_value(collab_resp.clone()).unwrap();
  let encode_collab: EncodedCollab = serde_json::from_value(json).unwrap();
  assert_eq!(collab_resp.encode_collab, encode_collab);
}

#[tokio::test]
async fn batch_insert_collab_with_empty_payload_test() {
  let mut test_client = TestClient::new_user().await;
  let workspace_id = test_client.workspace_id().await;

  let error = test_client
    .create_collab_list(&workspace_id, vec![])
    .await
    .unwrap_err();

  assert_eq!(error.code, ErrorCode::InvalidRequest);
}

#[tokio::test]
async fn create_collab_params_compatibility_serde_test() {
  // This test is to make sure that the CreateCollabParams is compatible with the old InsertCollabParams
  let object_id = uuid::Uuid::new_v4();
  let encoded_collab_v1 = default_document_collab_data(&object_id.to_string(), default_client_id())
    .unwrap()
    .encode_to_bytes()
    .unwrap();

  let old_version_value = json!(InsertCollabParams {
    object_id,
    encoded_collab_v1: encoded_collab_v1.clone(),
    workspace_id: WORKSPACE_ID,
    collab_type: CollabType::Document,
  });

  let new_version_create_params =
    serde_json::from_value::<CreateCollabParams>(old_version_value.clone()).unwrap();

  let new_version_value = serde_json::to_value(new_version_create_params.clone()).unwrap();
  assert_json_include!(actual: new_version_value.clone(), expected: old_version_value.clone());

  assert_eq!(new_version_create_params.object_id, object_id);
  assert_eq!(
    new_version_create_params.encoded_collab_v1,
    encoded_collab_v1
  );
  assert_eq!(new_version_create_params.workspace_id, WORKSPACE_ID);
  assert_eq!(new_version_create_params.collab_type, CollabType::Document);
}

#[derive(Serialize)]
struct InsertCollabParams {
  pub object_id: Uuid,
  pub encoded_collab_v1: Vec<u8>,
  pub workspace_id: Uuid,
  pub collab_type: CollabType,
}

#[tokio::test]
async fn create_collab_compatibility_with_json_params_test() {
  let test_client = TestClient::new_user().await;
  let workspace_id = test_client.workspace_id().await;
  let object_id = Uuid::new_v4();
  let api_client = &test_client.api_client;
  let url = format!(
    "{}/api/workspace/{}/collab/{}",
    api_client.base_url, workspace_id, &object_id
  );

  let encoded_collab = test_encode_collab_v1(&object_id, "title", "hello world");
  let params = OldCreateCollabParams {
    inner: CreateCollabData {
      object_id,
      encoded_collab_v1: encoded_collab.encode_to_bytes().unwrap().into(),
      collab_type: CollabType::Unknown,
    },
    workspace_id,
  };

  test_client
    .api_client
    .http_client_with_auth(Method::POST, &url)
    .await
    .unwrap()
    .json(&params)
    .send()
    .await
    .unwrap();

  let resp = test_client
    .api_client
    .http_client_with_auth(Method::GET, &url)
    .await
    .unwrap()
    .json(&QueryCollabParams {
      workspace_id,
      inner: QueryCollab {
        object_id,
        collab_type: CollabType::Unknown,
      },
    })
    .send()
    .await
    .unwrap();

  let encoded_collab_from_server = process_response_data::<EncodedCollab>(resp).await.unwrap();
  assert_eq!(encoded_collab, encoded_collab_from_server);
}

#[tokio::test]
async fn batch_insert_document_collab_test() {
  let mut test_client = TestClient::new_user().await;
  let workspace_id = test_client.workspace_id().await;

  let num_collabs = 100;
  let mut list = vec![];
  for _ in 0..num_collabs {
    let object_id = Uuid::new_v4();
    let mut editor = empty_document_editor(&object_id);
    let paragraphs = vec![
      generate_random_string(1),
      generate_random_string(2),
      generate_random_string(5),
    ];
    editor.insert_paragraphs(paragraphs);
    list.push((object_id, editor.encode_collab()));
  }

  let params_list = list
    .iter()
    .map(|(object_id, encoded_collab_v1)| CollabParams {
      object_id: *object_id,
      encoded_collab_v1: encoded_collab_v1.encode_to_bytes().unwrap().into(),
      collab_type: CollabType::Document,
      updated_at: None,
    })
    .collect::<Vec<_>>();

  test_client
    .create_collab_list(&workspace_id, params_list.clone())
    .await
    .unwrap();

  let params = params_list
    .iter()
    .map(|params| QueryCollab {
      object_id: params.object_id,
      collab_type: params.collab_type,
    })
    .collect::<Vec<_>>();

  let result = test_client
    .batch_get_collab(&workspace_id, params)
    .await
    .unwrap();

  for params in params_list {
    let encoded_collab = result.0.get(&params.object_id).unwrap();
    match encoded_collab {
      QueryCollabResult::Success { encode_collab_v1 } => {
        let actual = EncodedCollab::decode_from_bytes(encode_collab_v1.as_ref()).unwrap();
        let expected = EncodedCollab::decode_from_bytes(params.encoded_collab_v1.as_ref()).unwrap();
        assert_eq!(actual.doc_state, expected.doc_state);
      },
      QueryCollabResult::Failed { error } => {
        panic!("Failed to get collab: {:?}", error);
      },
    }
  }

  assert_eq!(result.0.values().len(), num_collabs);
}

#[derive(Debug, Clone, Serialize)]
pub struct OldCreateCollabParams {
  #[serde(flatten)]
  inner: CreateCollabData,
  pub workspace_id: Uuid,
}
