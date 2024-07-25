use std::thread::sleep;
use std::time::Duration;

use app_error::ErrorCode;
use client_api::entity::{AFRole, GlobalComment, PublishCollabItem, PublishCollabMetadata};
use client_api_test::TestClient;
use client_api_test::{generate_unique_registered_user_client, localhost_client};
use itertools::Itertools;

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
  let (page_owner_client, page_owner) = generate_unique_registered_user_client().await;
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

  // Test if only authenticated users can create
  let page_owner_comment_content = "comment from page owner";
  page_owner_client
    .create_comment_on_published_view(&view_id, page_owner_comment_content, &None)
    .await
    .unwrap();
  let (first_user_client, first_user) = generate_unique_registered_user_client().await;
  let first_user_comment_content = "comment from first authenticated user";
  // This is to ensure that the second comment creation timestamp is later than the first one
  sleep(Duration::from_millis(1));
  first_user_client
    .create_comment_on_published_view(&view_id, first_user_comment_content, &None)
    .await
    .unwrap();
  let guest_client = localhost_client();
  let result = guest_client
    .create_comment_on_published_view(&view_id, "comment from anonymous", &None)
    .await;
  assert!(result.is_err());
  assert_eq!(result.unwrap_err().code, ErrorCode::NotLoggedIn);

  // Test if only all users, authenticated or not, can view all the comments
  let published_view_comments: Vec<GlobalComment> = page_owner_client
    .get_published_view_comments(&view_id)
    .await
    .unwrap()
    .comments;
  assert_eq!(published_view_comments.len(), 2);
  let published_view_comments: Vec<GlobalComment> = first_user_client
    .get_published_view_comments(&view_id)
    .await
    .unwrap()
    .comments;
  assert_eq!(published_view_comments.len(), 2);
  let mut published_view_comments: Vec<GlobalComment> = guest_client
    .get_published_view_comments(&view_id)
    .await
    .unwrap()
    .comments;
  assert_eq!(published_view_comments.len(), 2);
  assert!(published_view_comments.iter().all(|c| !c.is_deleted));

  // Test if the comments have the correct content when sorted by creation time
  published_view_comments.sort_by_key(|c| c.created_at);
  let comment_creators = published_view_comments
    .iter()
    .map(|c| {
      c.user
        .as_ref()
        .map(|u| u.name.clone())
        .unwrap_or("".to_string())
    })
    .collect_vec();
  assert_eq!(
    comment_creators,
    vec![page_owner.email.clone(), first_user.email.clone()]
  );
  let comment_content = published_view_comments
    .iter()
    .map(|c| c.content.clone())
    .collect_vec();
  assert_eq!(
    comment_content,
    vec![page_owner_comment_content, first_user_comment_content]
  );

  // Test if it's possible to reply to another user's comment
  let second_user_comment_content = "comment from second authenticated user";
  let (second_user_client, second_user) = generate_unique_registered_user_client().await;
  // User 2 reply to user 1
  second_user_client
    .create_comment_on_published_view(
      &view_id,
      second_user_comment_content,
      &Some(published_view_comments[1].comment_id),
    )
    .await
    .unwrap();
  let mut published_view_comments: Vec<GlobalComment> = guest_client
    .get_published_view_comments(&view_id)
    .await
    .unwrap()
    .comments;
  published_view_comments.sort_by_key(|c| c.created_at);
  let comment_creators = published_view_comments
    .iter()
    .map(|c| {
      c.user
        .as_ref()
        .map(|u| u.name.clone())
        .unwrap_or("".to_string())
    })
    .collect_vec();
  assert_eq!(
    comment_creators,
    vec![
      page_owner.email.clone(),
      first_user.email.clone(),
      second_user.email.clone()
    ]
  );
  assert_eq!(
    published_view_comments[2].reply_comment_id,
    Some(published_view_comments[1].comment_id)
  );

  // Test if only the page owner or the comment creator can delete a comment
  // User 1 attempt to delete page owner's comment
  let result = first_user_client
    .delete_comment_on_published_view(&view_id, &published_view_comments[0].comment_id)
    .await;
  assert!(result.is_err());
  assert_eq!(result.unwrap_err().code, ErrorCode::UserUnAuthorized);
  // User 1 deletes own comment
  first_user_client
    .delete_comment_on_published_view(&view_id, &published_view_comments[1].comment_id)
    .await
    .unwrap();
  // Guest client attempt to delete user 2's comment
  let result = guest_client
    .delete_comment_on_published_view(&view_id, &published_view_comments[2].comment_id)
    .await;
  assert!(result.is_err());
  assert_eq!(result.unwrap_err().code, ErrorCode::NotLoggedIn);
  // Verify that the comments are not deleted from the database, only the is_deleted status changes.
  let mut published_view_comments: Vec<GlobalComment> = guest_client
    .get_published_view_comments(&view_id)
    .await
    .unwrap()
    .comments;
  published_view_comments.sort_by_key(|c| c.created_at);
  assert_eq!(
    published_view_comments
      .iter()
      .map(|c| c.is_deleted)
      .collect_vec(),
    vec![false, true, false]
  );
  // Verify that the reference id is still preserved
  assert_eq!(
    published_view_comments[2].reply_comment_id,
    Some(published_view_comments[1].comment_id)
  );

  for comment in &published_view_comments {
    page_owner_client
      .delete_comment_on_published_view(&view_id, &comment.comment_id)
      .await
      .unwrap();
  }

  let published_view_comments: Vec<GlobalComment> = guest_client
    .get_published_view_comments(&view_id)
    .await
    .unwrap()
    .comments;
  assert_eq!(published_view_comments.len(), 3);
  assert!(published_view_comments.iter().all(|c| c.is_deleted));
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
