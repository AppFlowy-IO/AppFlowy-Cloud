use app_error::ErrorCode;
use client_api::entity::{AFRole, GlobalComment, PublishCollabItem, PublishCollabMetadata};
use client_api_test::TestClient;
use client_api_test::{generate_unique_registered_user_client, localhost_client};
use uuid::Uuid;

#[tokio::test]
async fn test_set_publish_namespace_set() {
  let (c, _user) = generate_unique_registered_user_client().await;
  let workspace_id = get_first_workspace_string(&c).await;

  {
    // can get namespace before setting, which is random string
    let _ = c
      .get_workspace_publish_namespace(&workspace_id.to_string())
      .await
      .unwrap();
  }

  let namespace = uuid::Uuid::new_v4().to_string();
  c.set_workspace_publish_namespace(&workspace_id.to_string(), &namespace)
    .await
    .unwrap();

  {
    // cannot set the same namespace
    let err = c
      .set_workspace_publish_namespace(&workspace_id.to_string(), &namespace)
      .await
      .err()
      .unwrap();
    assert_eq!(format!("{:?}", err.code), "PublishNamespaceAlreadyTaken");
  }
  {
    // can replace the namespace
    let namespace = uuid::Uuid::new_v4().to_string();
    c.set_workspace_publish_namespace(&workspace_id.to_string(), &namespace)
      .await
      .unwrap();

    let got_namespace = c
      .get_workspace_publish_namespace(&workspace_id.to_string())
      .await
      .unwrap();
    assert_eq!(got_namespace, namespace);
  }
  {
    // cannot set namespace too short
    let err = c
      .set_workspace_publish_namespace(&workspace_id.to_string(), "a") // too short
      .await
      .err()
      .unwrap();
    assert_eq!(format!("{:?}", err.code), "InvalidRequest");
  }

  {
    // cannot set namespace with invalid chars
    let err = c
      .set_workspace_publish_namespace(&workspace_id.to_string(), "/|(*&)(&#@!") // invalid chars
      .await
      .err()
      .unwrap();
    assert_eq!(format!("{:?}", err.code), "InvalidRequest");
  }
}

#[tokio::test]
async fn test_publish_doc() {
  let (c, _user) = generate_unique_registered_user_client().await;
  let workspace_id = get_first_workspace_string(&c).await;
  let my_namespace = uuid::Uuid::new_v4().to_string();
  c.set_workspace_publish_namespace(&workspace_id.to_string(), &my_namespace)
    .await
    .unwrap();

  let publish_name_1 = "publish-name-1";
  let view_id_1 = uuid::Uuid::new_v4();
  let publish_name_2 = "publish-name-2";
  let view_id_2 = uuid::Uuid::new_v4();
  c.publish_collabs::<MyCustomMetadata, &[u8]>(
    &workspace_id,
    vec![
      PublishCollabItem {
        meta: PublishCollabMetadata {
          view_id: view_id_1,
          publish_name: publish_name_1.to_string(),
          metadata: MyCustomMetadata {
            title: "my_title_1".to_string(),
          },
        },
        data: "yrs_encoded_data_1".as_bytes(),
      },
      PublishCollabItem {
        meta: PublishCollabMetadata {
          view_id: view_id_2,
          publish_name: publish_name_2.to_string(),
          metadata: MyCustomMetadata {
            title: "my_title_2".to_string(),
          },
        },
        data: "yrs_encoded_data_2".as_bytes(),
      },
    ],
  )
  .await
  .unwrap();

  {
    // Non login user should be able to view the published collab
    let guest_client = localhost_client();
    let published_collab = guest_client
      .get_published_collab::<MyCustomMetadata>(&my_namespace, publish_name_1)
      .await
      .unwrap();
    assert_eq!(published_collab.title, "my_title_1");

    let publish_info = guest_client
      .get_published_collab_info(&view_id_1)
      .await
      .unwrap();
    assert_eq!(publish_info.namespace, Some(my_namespace.clone()));
    assert_eq!(publish_info.publish_name, publish_name_1);
    assert_eq!(publish_info.view_id, view_id_1);

    let blob = guest_client
      .get_published_collab_blob(&my_namespace, publish_name_1)
      .await
      .unwrap();
    assert_eq!(blob, "yrs_encoded_data_1");

    let publish_info = guest_client
      .get_published_collab_info(&view_id_2)
      .await
      .unwrap();
    assert_eq!(publish_info.namespace, Some(my_namespace.clone()));
    assert_eq!(publish_info.publish_name, publish_name_2);
    assert_eq!(publish_info.view_id, view_id_2);

    let blob = guest_client
      .get_published_collab_blob(&my_namespace, publish_name_2)
      .await
      .unwrap();
    assert_eq!(blob, "yrs_encoded_data_2");
  }

  // updates data
  c.publish_collabs::<MyCustomMetadata, &[u8]>(
    &workspace_id,
    vec![
      PublishCollabItem {
        meta: PublishCollabMetadata {
          view_id: view_id_1,
          publish_name: publish_name_1.to_string(),
          metadata: MyCustomMetadata {
            title: "my_title_1".to_string(),
          },
        },
        data: "yrs_encoded_data_3".as_bytes(),
      },
      PublishCollabItem {
        meta: PublishCollabMetadata {
          view_id: view_id_2,
          publish_name: publish_name_2.to_string(),
          metadata: MyCustomMetadata {
            title: "my_title_2".to_string(),
          },
        },
        data: "yrs_encoded_data_4".as_bytes(),
      },
    ],
  )
  .await
  .unwrap();

  {
    // should see updated data
    let guest_client = localhost_client();
    let blob = guest_client
      .get_published_collab_blob(&my_namespace, publish_name_1)
      .await
      .unwrap();
    assert_eq!(blob, "yrs_encoded_data_3");

    let blob = guest_client
      .get_published_collab_blob(&my_namespace, publish_name_2)
      .await
      .unwrap();
    assert_eq!(blob, "yrs_encoded_data_4");
  }

  c.unpublish_collabs(&workspace_id, &[view_id_1])
    .await
    .unwrap();

  {
    // Deleted collab should not be accessible
    let guest_client = localhost_client();
    let err = guest_client
      .get_published_collab::<MyCustomMetadata>(&my_namespace, publish_name_1)
      .await
      .err()
      .unwrap();
    assert_eq!(format!("{:?}", err.code), "RecordNotFound");

    let guest_client = localhost_client();
    let err = guest_client
      .get_published_collab_blob(&my_namespace, publish_name_1)
      .await
      .err()
      .unwrap();
    assert_eq!(format!("{:?}", err.code), "RecordNotFound");
  }
}

#[tokio::test]
async fn test_publish_comments() {
  let (page_owner_client, _) = generate_unique_registered_user_client().await;
  let workspace_id = get_first_workspace_string(&page_owner_client).await;
  let published_view_namespace = uuid::Uuid::new_v4().to_string();
  page_owner_client
    .set_workspace_publish_namespace(&workspace_id.to_string(), &published_view_namespace)
    .await
    .unwrap();

  let publish_name = "published-view";
  let view_id = uuid::Uuid::new_v4();
  page_owner_client
    .publish_collabs::<MyCustomMetadata, &[u8]>(
      &workspace_id,
      vec![PublishCollabItem {
        meta: PublishCollabMetadata {
          view_id,
          publish_name: publish_name.to_string(),
          metadata: MyCustomMetadata {
            title: "some_title".to_string(),
          },
        },
        data: "yrs_encoded_data_1".as_bytes(),
      }],
    )
    .await
    .unwrap();
  // TODO: replace the placeholder with actual comment id once the API implementation is completed
  let place_holder_comment_id = Uuid::new_v4();
  let published_view_comments: Vec<GlobalComment> = page_owner_client
    .get_published_view_comments(&view_id)
    .await
    .unwrap()
    .0;
  assert_eq!(published_view_comments.len(), 0);
  page_owner_client
    .create_comment_on_published_view(&view_id, "comment from page owner")
    .await
    .unwrap();
  page_owner_client
    .delete_comment_on_published_view(&view_id, &place_holder_comment_id)
    .await
    .unwrap();
  let guest_client = localhost_client();
  let published_view_comments: Vec<GlobalComment> = guest_client
    .get_published_view_comments(&view_id)
    .await
    .unwrap()
    .0;
  assert_eq!(published_view_comments.len(), 0);
  let guest_client = localhost_client();
  let result = guest_client
    .create_comment_on_published_view(&view_id, "comment from anonymous")
    .await;
  assert!(result.is_err());
  assert_eq!(result.unwrap_err().code, ErrorCode::NotLoggedIn);
  let result = guest_client
    .delete_comment_on_published_view(&view_id, &place_holder_comment_id)
    .await;
  assert!(result.is_err());
  assert_eq!(result.unwrap_err().code, ErrorCode::NotLoggedIn);

  let (authenticated_user_client, _) = generate_unique_registered_user_client().await;
  authenticated_user_client
    .create_comment_on_published_view(&view_id, "comment from authenticated user")
    .await
    .unwrap();
  let result = authenticated_user_client
    .delete_comment_on_published_view(&view_id, &place_holder_comment_id)
    .await;
  assert!(result.is_err());
  assert_eq!(result.unwrap_err().code, ErrorCode::UserUnAuthorized)
}

#[tokio::test]
async fn test_publish_load_test() {
  let (c, _user) = generate_unique_registered_user_client().await;
  let workspace_id = get_first_workspace_string(&c).await;
  let my_namespace = uuid::Uuid::new_v4().to_string();
  c.set_workspace_publish_namespace(&workspace_id.to_string(), &my_namespace)
    .await
    .unwrap();

  // publish nothing
  c.publish_collabs::<(), &[u8]>(&workspace_id, vec![])
    .await
    .unwrap();

  // publish nothing with metadata
  c.publish_collabs::<MyCustomMetadata, &[u8]>(&workspace_id, vec![])
    .await
    .unwrap();

  // publish 1000 collabs
  let collabs: Vec<PublishCollabItem<MyCustomMetadata, Vec<u8>>> = (0..1000)
    .map(|i| PublishCollabItem {
      meta: PublishCollabMetadata {
        view_id: uuid::Uuid::new_v4(),
        publish_name: format!("publish-name-{}", i),
        metadata: MyCustomMetadata {
          title: format!("title_{}", i),
        },
      },
      data: vec![0; 100_000], // 100 KB
    })
    .collect();

  c.publish_collabs(&workspace_id, collabs).await.unwrap();
}

async fn get_first_workspace_string(c: &client_api::Client) -> String {
  c.get_workspaces()
    .await
    .unwrap()
    .0
    .first()
    .unwrap()
    .workspace_id
    .to_string()
}

#[tokio::test]
async fn workspace_member_publish_unpublish() {
  let client_1 = TestClient::new_user_without_ws_conn().await;
  let workspace_id = client_1.workspace_id().await;
  let client_2 = TestClient::new_user_without_ws_conn().await;
  client_1
    .invite_and_accepted_workspace_member(&workspace_id, &client_2, AFRole::Member)
    .await
    .unwrap();

  let view_id = uuid::Uuid::new_v4();
  // member can publish without owner setting namespace
  client_2
    .api_client
    .publish_collabs::<MyCustomMetadata, &[u8]>(
      &workspace_id,
      vec![PublishCollabItem {
        meta: PublishCollabMetadata {
          view_id,
          publish_name: "publish-name-1".to_string(),
          metadata: MyCustomMetadata {
            title: "my_title_1".to_string(),
          },
        },
        data: "yrs_encoded_data_1".as_bytes(),
      }],
    )
    .await
    .unwrap();

  client_2
    .api_client
    .unpublish_collabs(&workspace_id, &[view_id])
    .await
    .unwrap();
}

#[derive(serde::Serialize, serde::Deserialize)]
struct MyCustomMetadata {
  title: String,
}
