use app_error::ErrorCode;
use assert_json_diff::assert_json_include;
use collab::entity::EncodedCollab;
use collab_document::document_data::default_document_collab_data;
use collab_entity::CollabType;
use database_entity::dto::{
  BatchCreateCollabParams, CollabParams, CreateCollabParams, QueryCollab, QueryCollabParams,
  QueryCollabResult,
};

use reqwest::Method;
use serde::Serialize;
use serde_json::json;

use crate::collab::util::{generate_random_string, test_encode_collab_v1};
use client_api_test::TestClient;
use shared_entity::response::AppResponse;
use uuid::Uuid;

#[tokio::test]
async fn get_collab_response_compatible_test() {
  let mut test_client = TestClient::new_user().await;
  let workspace_id = test_client.workspace_id().await;

  let params = QueryCollabParams {
    workspace_id: workspace_id.clone(),
    inner: QueryCollab {
      object_id: workspace_id.clone(),
      collab_type: CollabType::Folder,
    },
  };
  // after 0.3.22, we use [CollabResponse] instead of EncodedCollab as the response
  let data = test_client.get_collab(params).await.unwrap();
  assert_eq!(data.object_id, workspace_id);

  let json = serde_json::to_value(data.clone()).unwrap();
  let encode_collab: EncodedCollab = serde_json::from_value(json).unwrap();
  assert_eq!(data.encode_collab, encode_collab);
}

#[tokio::test]
#[should_panic]
async fn create_collab_workspace_id_equal_to_object_id_test() {
  let mut test_client = TestClient::new_user().await;
  let workspace_id = test_client.workspace_id().await;
  // Only the object with [CollabType::Folder] can have the same object_id as workspace_id. But
  // it should use create workspace API
  test_client
    .create_collab_with_data(
      workspace_id.clone(),
      &workspace_id,
      CollabType::Unknown,
      None,
    )
    .await
    .unwrap()
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
async fn batch_insert_collab_success_test() {
  let mut test_client = TestClient::new_user().await;
  let workspace_id = test_client.workspace_id().await;

  let mock_encoded_collab_v1 = vec![
    test_encode_collab_v1("1", "title", &generate_random_string(1024)),
    test_encode_collab_v1("2", "title", &generate_random_string(3 * 1024)),
    test_encode_collab_v1("3", "title", &generate_random_string(600 * 1024)),
    test_encode_collab_v1("4", "title", &generate_random_string(800 * 1024)),
    test_encode_collab_v1("5", "title", &generate_random_string(1024 * 1024)),
  ];

  let params_list = (0..5)
    .map(|i| CollabParams {
      object_id: Uuid::new_v4().to_string(),
      encoded_collab_v1: mock_encoded_collab_v1[i].encode_to_bytes().unwrap(),
      collab_type: CollabType::Unknown,
    })
    .collect::<Vec<_>>();

  test_client
    .create_collab_list(&workspace_id, params_list.clone())
    .await
    .unwrap();

  let params = params_list
    .iter()
    .map(|params| QueryCollab {
      object_id: params.object_id.clone(),
      collab_type: params.collab_type.clone(),
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
        assert_eq!(encode_collab_v1, &params.encoded_collab_v1)
      },
      QueryCollabResult::Failed { .. } => {
        panic!("Failed to get collab");
      },
    }
  }

  assert_eq!(result.0.values().len(), 5);
}

#[tokio::test]
async fn create_collab_params_compatibility_serde_test() {
  // This test is to make sure that the CreateCollabParams is compatible with the old InsertCollabParams
  let object_id = uuid::Uuid::new_v4().to_string();
  let encoded_collab_v1 = default_document_collab_data(&object_id)
    .unwrap()
    .encode_to_bytes()
    .unwrap();

  let old_version_value = json!(InsertCollabParams {
    object_id: object_id.clone(),
    encoded_collab_v1: encoded_collab_v1.clone(),
    workspace_id: "workspace_id".to_string(),
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
  assert_eq!(
    new_version_create_params.workspace_id,
    "workspace_id".to_string()
  );
  assert_eq!(new_version_create_params.collab_type, CollabType::Document);
}

#[derive(Serialize)]
struct InsertCollabParams {
  pub object_id: String,
  pub encoded_collab_v1: Vec<u8>,
  pub workspace_id: String,
  pub collab_type: CollabType,
}

#[tokio::test]
async fn create_collab_compatibility_with_json_params_test() {
  let test_client = TestClient::new_user().await;
  let workspace_id = test_client.workspace_id().await;
  let object_id = Uuid::new_v4().to_string();
  let api_client = &test_client.api_client;
  let url = format!(
    "{}/api/workspace/{}/collab/{}",
    api_client.base_url, workspace_id, &object_id
  );

  let encoded_collab = test_encode_collab_v1(&object_id, "title", "hello world");
  let params = OldCreateCollabParams {
    inner: CollabParams {
      object_id: object_id.clone(),
      encoded_collab_v1: encoded_collab.encode_to_bytes().unwrap(),
      collab_type: CollabType::Unknown,
    },
    workspace_id: workspace_id.clone(),
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
        object_id: object_id.clone(),
        collab_type: CollabType::Unknown,
      },
    })
    .send()
    .await
    .unwrap();

  let encoded_collab_from_server = AppResponse::<EncodedCollab>::from_response(resp)
    .await
    .unwrap()
    .into_data()
    .unwrap();
  assert_eq!(encoded_collab, encoded_collab_from_server);
}

#[tokio::test]
async fn batch_create_collab_compatibility_with_uncompress_params_test() {
  let test_client = TestClient::new_user().await;
  let workspace_id = test_client.workspace_id().await;
  let object_id = Uuid::new_v4().to_string();
  let api_client = &test_client.api_client;
  let url = format!(
    "{}/api/workspace/{}/collabs",
    api_client.base_url, workspace_id,
  );

  let encoded_collab = test_encode_collab_v1(&object_id, "title", "hello world");
  let params = BatchCreateCollabParams {
    workspace_id: workspace_id.to_string(),
    params_list: vec![CollabParams {
      object_id: object_id.clone(),
      encoded_collab_v1: encoded_collab.encode_to_bytes().unwrap(),
      collab_type: CollabType::Unknown,
    }],
  }
  .to_bytes()
  .unwrap();

  test_client
    .api_client
    .http_client_with_auth(Method::POST, &url)
    .await
    .unwrap()
    .body(params)
    .send()
    .await
    .unwrap();

  let url = format!(
    "{}/api/workspace/{}/collab/{}",
    api_client.base_url, workspace_id, &object_id
  );
  let resp = test_client
    .api_client
    .http_client_with_auth(Method::GET, &url)
    .await
    .unwrap()
    .json(&QueryCollabParams {
      workspace_id,
      inner: QueryCollab {
        object_id: object_id.clone(),
        collab_type: CollabType::Unknown,
      },
    })
    .send()
    .await
    .unwrap();

  let encoded_collab_from_server = AppResponse::<EncodedCollab>::from_response(resp)
    .await
    .unwrap()
    .into_data()
    .unwrap();
  assert_eq!(encoded_collab, encoded_collab_from_server);
}

#[derive(Debug, Clone, Serialize)]
pub struct OldCreateCollabParams {
  #[serde(flatten)]
  inner: CollabParams,
  pub workspace_id: String,
}
