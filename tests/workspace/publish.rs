use appflowy_cloud::biz::collab::folder_view::collab_folder_to_folder_view;
use collab_entity::CollabType;
use collab_folder::{CollabOrigin, Folder};
use shared_entity::dto::publish_dto::PublishViewMetaData;
use shared_entity::dto::workspace_dto::PublishedDuplicate;
use std::collections::HashMap;
use std::thread::sleep;
use std::time::Duration;

use app_error::ErrorCode;
use client_api::entity::{
  AFRole, GlobalComment, PublishCollabItem, PublishCollabMetadata, QueryCollab, QueryCollabParams,
};
use client_api_test::TestClient;
use client_api_test::{generate_unique_registered_user_client, localhost_client};
use itertools::Itertools;

use crate::workspace::published_data::{self};

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

  // Test if only all users, authenticated or not, can view all the comments,
  // and whether the `can_be_deleted` field is correctly set
  let published_view_comments: Vec<GlobalComment> = page_owner_client
    .get_published_view_comments(&view_id)
    .await
    .unwrap()
    .comments;
  assert_eq!(published_view_comments.len(), 2);
  assert!(published_view_comments.iter().all(|c| c.can_be_deleted));
  let published_view_comments: Vec<GlobalComment> = first_user_client
    .get_published_view_comments(&view_id)
    .await
    .unwrap()
    .comments;
  assert_eq!(published_view_comments.len(), 2);
  assert_eq!(
    published_view_comments
      .iter()
      .map(|c| c.can_be_deleted)
      .collect_vec(),
    vec![true, false]
  );
  let published_view_comments: Vec<GlobalComment> = guest_client
    .get_published_view_comments(&view_id)
    .await
    .unwrap()
    .comments;
  assert_eq!(published_view_comments.len(), 2);
  assert!(published_view_comments.iter().all(|c| !c.can_be_deleted));

  // Test if the comments are correctly sorted
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
    vec![first_user.email.clone(), page_owner.email.clone()]
  );
  let comment_content = published_view_comments
    .iter()
    .map(|c| c.content.clone())
    .collect_vec();
  assert_eq!(
    comment_content,
    vec![first_user_comment_content, page_owner_comment_content]
  );

  // Test if it's possible to reply to another user's comment
  let second_user_comment_content = "comment from second authenticated user";
  let (second_user_client, second_user) = generate_unique_registered_user_client().await;
  // User 2 reply to user 1
  second_user_client
    .create_comment_on_published_view(
      &view_id,
      second_user_comment_content,
      &Some(published_view_comments[0].comment_id),
    )
    .await
    .unwrap();
  let published_view_comments: Vec<GlobalComment> = guest_client
    .get_published_view_comments(&view_id)
    .await
    .unwrap()
    .comments;
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
      second_user.email.clone(),
      first_user.email.clone(),
      page_owner.email.clone()
    ]
  );
  assert_eq!(
    published_view_comments[0].reply_comment_id,
    Some(published_view_comments[1].comment_id)
  );

  // Test if only the page owner or the comment creator can delete a comment
  // User 1 attempt to delete page owner's comment
  let result = first_user_client
    .delete_comment_on_published_view(&view_id, &published_view_comments[2].comment_id)
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
    .delete_comment_on_published_view(&view_id, &published_view_comments[0].comment_id)
    .await;
  assert!(result.is_err());
  assert_eq!(result.unwrap_err().code, ErrorCode::NotLoggedIn);
  // Verify that the comments are not deleted from the database, only the is_deleted status changes.
  let published_view_comments: Vec<GlobalComment> = guest_client
    .get_published_view_comments(&view_id)
    .await
    .unwrap()
    .comments;
  assert_eq!(
    published_view_comments
      .iter()
      .map(|c| c.is_deleted)
      .collect_vec(),
    vec![false, true, false]
  );
  // Verify that the reference id is still preserved
  assert_eq!(
    published_view_comments[0].reply_comment_id,
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
  assert!(published_view_comments.iter().all(|c| !c.can_be_deleted));
}

#[tokio::test]
async fn test_excessive_comment_length() {
  let (client, _) = generate_unique_registered_user_client().await;
  let workspace_id = get_first_workspace_string(&client).await;
  let published_view_namespace = uuid::Uuid::new_v4().to_string();
  client
    .set_workspace_publish_namespace(&workspace_id.to_string(), &published_view_namespace)
    .await
    .unwrap();

  let publish_name = "published-view";
  let view_id = uuid::Uuid::new_v4();
  client
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

  let resp = client
    .create_comment_on_published_view(&view_id, "a".repeat(5001).as_str(), &None)
    .await;
  assert!(resp.is_err());
  assert_eq!(resp.unwrap_err().code, ErrorCode::StringLengthLimitReached);
}

#[tokio::test]
async fn test_publish_reactions() {
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
  page_owner_client
    .create_comment_on_published_view(&view_id, "likable comment", &None)
    .await
    .unwrap();
  // This is to ensure that the second comment creation timestamp is later than the first one
  sleep(Duration::from_millis(1));
  page_owner_client
    .create_comment_on_published_view(&view_id, "party comment", &None)
    .await
    .unwrap();
  let mut comments = page_owner_client
    .get_published_view_comments(&view_id)
    .await
    .unwrap()
    .comments;
  comments.sort_by_key(|c| c.created_at);
  // Test if the reactions are created correctly based on view and comment id
  let likable_comment_id = comments[0].comment_id;
  let party_comment_id = comments[1].comment_id;

  let like_emoji = "👍";
  let party_emoji = "🎉";
  page_owner_client
    .create_reaction_on_comment(like_emoji, &view_id, &likable_comment_id)
    .await
    .unwrap();
  let guest_client = localhost_client();
  let result = guest_client
    .create_reaction_on_comment(like_emoji, &view_id, &likable_comment_id)
    .await;
  assert!(result.is_err());
  assert_eq!(result.unwrap_err().code, ErrorCode::NotLoggedIn);

  let (user_client, _) = generate_unique_registered_user_client().await;
  sleep(Duration::from_millis(1));
  user_client
    .create_reaction_on_comment(party_emoji, &view_id, &party_comment_id)
    .await
    .unwrap();
  user_client
    .create_reaction_on_comment(like_emoji, &view_id, &likable_comment_id)
    .await
    .unwrap();

  let reactions = guest_client
    .get_published_view_reactions(&view_id, &None)
    .await
    .unwrap()
    .reactions;
  assert_eq!(reactions[0].reaction_type, like_emoji);
  assert_eq!(reactions[1].reaction_type, party_emoji);
  let reaction_count: HashMap<String, i32> = reactions
    .iter()
    .map(|r| (r.reaction_type.clone(), r.react_users.len() as i32))
    .collect();
  assert_eq!(reaction_count.len(), 2);
  assert_eq!(*reaction_count.get(like_emoji).unwrap(), 2);
  assert_eq!(*reaction_count.get(party_emoji).unwrap(), 1);

  // Test if the reactions are deleted correctly based on view and comment id
  let result = guest_client
    .delete_reaction_on_comment(like_emoji, &view_id, &likable_comment_id)
    .await;
  assert!(result.is_err());
  assert_eq!(result.unwrap_err().code, ErrorCode::NotLoggedIn);
  user_client
    .delete_reaction_on_comment(like_emoji, &view_id, &likable_comment_id)
    .await
    .unwrap();

  let reactions = guest_client
    .get_published_view_reactions(&view_id, &None)
    .await
    .unwrap()
    .reactions;
  let reaction_count: HashMap<String, i32> = reactions
    .iter()
    .map(|r| (r.reaction_type.clone(), r.react_users.len() as i32))
    .collect();
  assert_eq!(reaction_count.len(), 2);
  assert_eq!(*reaction_count.get(like_emoji).unwrap(), 1);
  assert_eq!(*reaction_count.get(party_emoji).unwrap(), 1);

  // Test if we can filter the reactions by comment id
  let reactions = guest_client
    .get_published_view_reactions(&view_id, &Some(likable_comment_id))
    .await
    .unwrap()
    .reactions;
  let reaction_count: HashMap<String, i32> = reactions
    .iter()
    .map(|r| (r.reaction_type.clone(), r.react_users.len() as i32))
    .collect();
  assert_eq!(reaction_count.len(), 1);
  assert_eq!(*reaction_count.get(like_emoji).unwrap(), 1);
}

#[tokio::test]
async fn test_publish_load_test() {
  let (c, _user) = generate_unique_registered_user_client().await;
  let workspace_id = get_first_workspace_string(&c).await;
  let my_namespace = uuid::Uuid::new_v4().to_string();
  c.set_workspace_publish_namespace(&workspace_id.to_string(), &my_namespace)
    .await
    .unwrap();

  {
    // cannot publish nothing
    let err = c
      .publish_collabs::<(), &[u8]>(&workspace_id, vec![])
      .await
      .unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidRequest);
  }

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

#[tokio::test]
async fn duplicate_to_workspace_references() {
  let client_1 = TestClient::new_user().await;
  let workspace_id = client_1.workspace_id().await;

  // doc2 contains a reference to doc1
  let doc_2_view_id = uuid::Uuid::new_v4();
  let doc_2_metadata: PublishViewMetaData =
    serde_json::from_str(published_data::DOC_2_META).unwrap();
  let doc_2_doc_state = hex::decode(published_data::DOC_2_DOC_STATE_HEX).unwrap();

  // doc_1_view_id needs to be fixed because doc_2 references it
  let doc_1_view_id: uuid::Uuid = "e8c4f99a-50ea-4758-bca0-afa7df5c2434".parse().unwrap();
  let doc_1_metadata: PublishViewMetaData =
    serde_json::from_str(published_data::DOC_1_META).unwrap();
  let doc_1_doc_state = hex::decode(published_data::DOC_1_DOC_STATE_HEX).unwrap();

  // doc1 contains @reference database to grid1 (not inline)
  let grid_1_view_id: uuid::Uuid = "8e062f61-d7ae-4f4b-869c-f44c43149399".parse().unwrap();
  let grid_1_metadata: PublishViewMetaData =
    serde_json::from_str(published_data::GRID_1_META).unwrap();
  let grid_1_db_data = hex::decode(published_data::GRID_1_DB_DATA).unwrap();

  client_1
    .api_client
    .publish_collabs(
      &workspace_id,
      vec![
        PublishCollabItem {
          meta: PublishCollabMetadata {
            view_id: doc_1_view_id,
            publish_name: doc_1_metadata.view.name.clone(),
            metadata: doc_1_metadata.clone(),
          },
          data: doc_1_doc_state,
        },
        PublishCollabItem {
          meta: PublishCollabMetadata {
            view_id: doc_2_view_id,
            publish_name: doc_2_metadata.view.name.clone(),
            metadata: doc_2_metadata.clone(),
          },
          data: doc_2_doc_state,
        },
        PublishCollabItem {
          meta: PublishCollabMetadata {
            view_id: grid_1_view_id,
            publish_name: grid_1_metadata.view.name.clone(),
            metadata: grid_1_metadata.clone(),
          },
          data: grid_1_db_data,
        },
      ],
    )
    .await
    .unwrap();

  {
    let client_2 = TestClient::new_user().await;
    let workspace_id_2 = client_2.workspace_id().await;
    let fv = client_2
      .api_client
      .get_workspace_folder(&workspace_id_2, Some(5))
      .await
      .unwrap();

    // duplicate doc2 to workspace2
    // Result fv should be:
    // .
    // ├── Getting Started (existing)
    // └── doc2
    //     └── doc1
    //         └── grid1
    client_2
      .api_client
      .duplicate_published_to_workspace(
        &workspace_id_2,
        &PublishedDuplicate {
          published_view_id: doc_2_view_id.to_string(),
          dest_view_id: fv.view_id, // use the root view
        },
      )
      .await
      .unwrap();

    let fv = client_2
      .api_client
      .get_workspace_folder(&workspace_id_2, Some(5))
      .await
      .unwrap();

    let doc_2_fv = fv
      .children
      .into_iter()
      .find(|v| v.name == doc_2_metadata.view.name)
      .unwrap();
    let doc_1_fv = doc_2_fv
      .children
      .into_iter()
      .find(|v| v.name == doc_1_metadata.view.name)
      .unwrap();
    let _grid_1_fv = doc_1_fv
      .children
      .into_iter()
      .find(|v| v.name == grid_1_metadata.view.name)
      .unwrap();
  }
}

#[tokio::test]
async fn duplicate_to_workspace_doc_inline_database() {
  let client_1 = TestClient::new_user().await;
  let workspace_id = client_1.workspace_id().await;

  // doc3 contains inline database to a view in grid1 (view of grid1)
  let doc_3_view_id = uuid::Uuid::new_v4();
  let doc_3_metadata: PublishViewMetaData =
    serde_json::from_str(published_data::DOC_3_META).unwrap();
  let doc_3_doc_state = hex::decode(published_data::DOC_3_DOC_STATE_HEX).unwrap();

  // view of grid1
  let view_of_grid_1_view_id: uuid::Uuid = "d8589e98-88fc-42e4-888c-b03338bf22bb".parse().unwrap();
  let view_of_grid_1_metadata: PublishViewMetaData =
    serde_json::from_str(published_data::VIEW_OF_GRID1_META).unwrap();
  let view_of_grid_1_db_data = hex::decode(published_data::VIEW_OF_GRID_1_DB_DATA).unwrap();

  client_1
    .api_client
    .publish_collabs(
      &workspace_id,
      vec![
        PublishCollabItem {
          meta: PublishCollabMetadata {
            view_id: doc_3_view_id,
            publish_name: doc_3_metadata.view.name.clone(),
            metadata: doc_3_metadata.clone(),
          },
          data: doc_3_doc_state,
        },
        PublishCollabItem {
          meta: PublishCollabMetadata {
            view_id: view_of_grid_1_view_id,
            publish_name: view_of_grid_1_metadata.view.name.replace(' ', "-"),
            metadata: view_of_grid_1_metadata.clone(),
          },
          data: view_of_grid_1_db_data,
        },
      ],
    )
    .await
    .unwrap();

  {
    let mut client_2 = TestClient::new_user().await;
    let workspace_id_2 = client_2.workspace_id().await;

    // Open workspace to trigger group creation
    client_2
      .open_collab(&workspace_id_2, &workspace_id_2, CollabType::Folder)
      .await;

    let fv = client_2
      .api_client
      .get_workspace_folder(&workspace_id_2, Some(5))
      .await
      .unwrap();

    // duplicate doc3 to workspace2
    // Result fv should be:
    // .
    // ├── Getting Started (existing)
    // └── doc3
    //     └── grid1
    //         └── View of grid1
    client_2
      .api_client
      .duplicate_published_to_workspace(
        &workspace_id_2,
        &PublishedDuplicate {
          published_view_id: doc_3_view_id.to_string(),
          dest_view_id: fv.view_id, // use the root view
        },
      )
      .await
      .unwrap();

    {
      let fv = client_2
        .api_client
        .get_workspace_folder(&workspace_id_2, Some(5))
        .await
        .unwrap();
      let doc_3_fv = fv
        .children
        .into_iter()
        .find(|v| v.name == doc_3_metadata.view.name)
        .unwrap();
      let grid1_fv = doc_3_fv
        .children
        .into_iter()
        .find(|v| v.name == "grid1")
        .unwrap();
      let _view_of_grid1_fv = grid1_fv
        .children
        .into_iter()
        .find(|v| v.name == "View of grid1")
        .unwrap();
    }

    {
      let collab_resp = client_2
        .get_collab(QueryCollabParams {
          workspace_id: workspace_id_2.clone(),
          inner: QueryCollab {
            object_id: workspace_id_2.clone(),
            collab_type: CollabType::Folder,
          },
        })
        .await
        .unwrap();

      let folder = Folder::from_collab_doc_state(
        client_2.uid().await,
        CollabOrigin::Server,
        collab_resp.encode_collab.into(),
        &workspace_id_2,
        vec![],
      )
      .unwrap();

      let folder_view = collab_folder_to_folder_view(&folder, 5);
      let doc_3_fv = folder_view
        .children
        .into_iter()
        .find(|v| v.name == doc_3_metadata.view.name)
        .unwrap();
      let grid1_fv = doc_3_fv
        .children
        .into_iter()
        .find(|v| v.name == "grid1")
        .unwrap();
      let _view_of_grid1_fv = grid1_fv
        .children
        .into_iter()
        .find(|v| v.name == "View of grid1")
        .unwrap();
    }
  }
}
