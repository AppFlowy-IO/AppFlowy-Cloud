use app_error::ErrorCode;
use appflowy_cloud::biz::collab::folder_view::collab_folder_to_folder_view;
use appflowy_cloud::biz::workspace::publish_dup::collab_from_doc_state;
use client_api::entity::{AFRole, GlobalComment, PublishCollabItem, PublishCollabMetadata};
use client_api_test::TestClient;
use client_api_test::{generate_unique_registered_user_client, localhost_client};
use collab::util::MapExt;
use collab_database::database::DatabaseBody;
use collab_database::entity::FieldType;
use collab_database::rows::RowDetail;
use collab_database::views::DatabaseViews;
use collab_database::workspace_database::WorkspaceDatabaseBody;
use collab_document::document::Document;
use collab_entity::CollabType;
use collab_folder::{CollabOrigin, Folder, UserId};
use itertools::Itertools;
use shared_entity::dto::publish_dto::PublishDatabaseData;
use shared_entity::dto::workspace_dto::PublishedDuplicate;
use std::collections::{HashMap, HashSet};
use std::thread::sleep;
use std::time::Duration;

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

  let doc_2_view_id = uuid::Uuid::new_v4();
  let doc_1_view_id: uuid::Uuid = "e8c4f99a-50ea-4758-bca0-afa7df5c2434".parse().unwrap();
  let grid_1_view_id: uuid::Uuid = "8e062f61-d7ae-4f4b-869c-f44c43149399".parse().unwrap();
  client_1
    .publish_collabs(
      &workspace_id,
      vec![
        (
          // doc2 contains a reference to doc1
          doc_2_view_id,
          published_data::DOC_2_META,
          published_data::DOC_2_DOC_STATE_HEX,
        ),
        (
          // doc_1_view_id needs to be fixed because doc_2 references it
          doc_1_view_id,
          published_data::DOC_1_META,
          published_data::DOC_1_DOC_STATE_HEX,
        ),
        (
          // doc1 contains @reference database to grid1 (not inline)
          grid_1_view_id,
          published_data::GRID_1_META,
          published_data::GRID_1_DB_DATA,
        ),
      ],
    )
    .await;

  {
    let client_2 = TestClient::new_user().await;
    let workspace_id_2 = client_2.workspace_id().await;
    let fv = client_2
      .api_client
      .get_workspace_folder(&workspace_id_2, Some(5), None)
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
      .get_workspace_folder(&workspace_id_2, Some(5), None)
      .await
      .unwrap();

    let doc_2_fv = fv.children.into_iter().find(|v| v.name == "doc2").unwrap();
    assert_ne!(doc_2_fv.view_id, doc_1_view_id.to_string());

    let doc_1_fv = doc_2_fv
      .children
      .into_iter()
      .find(|v| v.name == "doc1")
      .unwrap();
    assert_ne!(doc_1_fv.view_id, doc_1_view_id.to_string());

    let grid_1_fv = doc_1_fv
      .children
      .into_iter()
      .find(|v| v.name == "grid1")
      .unwrap();
    assert_ne!(grid_1_fv.view_id, grid_1_view_id.to_string());
  }
}

#[tokio::test]
async fn duplicate_to_workspace_doc_inline_database() {
  let client_1 = TestClient::new_user().await;
  let workspace_id = client_1.workspace_id().await;

  // doc3 contains inline database to a view in grid1 (view of grid1)
  let doc_3_view_id = uuid::Uuid::new_v4();

  // view of grid1
  let view_of_grid_1_view_id: uuid::Uuid = "d8589e98-88fc-42e4-888c-b03338bf22bb".parse().unwrap();
  let view_of_grid_1_db_data = hex::decode(published_data::VIEW_OF_GRID_1_DB_DATA).unwrap();
  let (pub_db_id, pub_row_ids) = get_database_id_and_row_ids(&view_of_grid_1_db_data);

  client_1
    .publish_collabs(
      &workspace_id,
      vec![
        (
          doc_3_view_id,
          published_data::DOC_3_META,
          published_data::DOC_3_DOC_STATE_HEX,
        ),
        (
          view_of_grid_1_view_id,
          published_data::VIEW_OF_GRID_1_META,
          published_data::VIEW_OF_GRID_1_DB_DATA,
        ),
      ],
    )
    .await;

  {
    let mut client_2 = TestClient::new_user().await;
    let workspace_id_2 = client_2.workspace_id().await;

    // Open workspace to trigger group creation
    client_2
      .open_collab(&workspace_id_2, &workspace_id_2, CollabType::Folder)
      .await;

    let fv = client_2
      .api_client
      .get_workspace_folder(&workspace_id_2, Some(5), None)
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
        .get_workspace_folder(&workspace_id_2, Some(5), None)
        .await
        .unwrap();
      let doc_3_fv = fv.children.into_iter().find(|v| v.name == "doc3").unwrap();
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

    let collab_resp = client_2
      .get_collab(
        workspace_id_2.clone(),
        workspace_id_2.clone(),
        CollabType::Folder,
      )
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

    let folder_view =
      collab_folder_to_folder_view(&workspace_id_2, &folder, 5, &HashSet::default()).unwrap();
    let doc_3_fv = folder_view
      .children
      .into_iter()
      .find(|v| v.name == "doc3")
      .unwrap();
    assert_ne!(doc_3_fv.view_id, doc_3_view_id.to_string());

    let grid1_fv = doc_3_fv
      .children
      .into_iter()
      .find(|v| v.name == "grid1")
      .unwrap();
    assert_ne!(grid1_fv.view_id, view_of_grid_1_view_id.to_string());

    let view_of_grid1_fv = grid1_fv
      .children
      .into_iter()
      .find(|v| v.name == "View of grid1")
      .unwrap();
    assert_ne!(view_of_grid1_fv.view_id, view_of_grid_1_view_id.to_string());

    {
      // check that database_id is different
      let mut ws_db_collab = client_2
        .get_workspace_database_collab(&workspace_id_2)
        .await;
      let ws_db_body = WorkspaceDatabaseBody::open(&mut ws_db_collab);
      let txn = ws_db_collab.transact();
      let dup_grid1_db_id = ws_db_body
        .get_all_database_meta(&txn)
        .into_iter()
        .find(|db_meta| db_meta.linked_views.contains(&view_of_grid1_fv.view_id))
        .unwrap()
        .database_id;
      let db_collab = client_2
        .get_collab_to_collab(workspace_id_2, dup_grid1_db_id, CollabType::Database)
        .await
        .unwrap();
      let dup_db_id = DatabaseBody::database_id_from_collab(&db_collab).unwrap();
      assert_ne!(dup_db_id, pub_db_id);

      let view_map = {
        let map_ref = db_collab
          .data
          .get_with_path(&txn, ["database", "views"])
          .unwrap();
        DatabaseViews::new(CollabOrigin::Empty, map_ref, None)
      };

      for db_view in view_map.get_all_views(&txn) {
        assert_eq!(db_view.database_id, dup_db_id);
        for row_order in db_view.row_orders {
          assert!(
            !pub_row_ids.contains(row_order.id.as_str()),
            "published row id is same as duplicated row id"
          );
        }
      }
    }
  }
}

#[tokio::test]
async fn duplicate_to_workspace_db_embedded_in_doc() {
  let client_1 = TestClient::new_user().await;
  let workspace_id = client_1.workspace_id().await;

  // embedded doc with db
  // database is created in the doc, not linked from a separate view
  let doc_with_embedded_db_view_id: uuid::Uuid = uuid::Uuid::new_v4();

  client_1
    .publish_collabs(
      &workspace_id,
      vec![
        (
          doc_with_embedded_db_view_id,
          published_data::DOC_WITH_EMBEDDED_DB_META,
          published_data::DOC_WITH_EMBEDDED_DB_HEX,
        ),
        (
          // user will also need to publish the database (even though it is embedded)
          // uuid must be fixed because it is referenced in the doc
          "bb221175-14da-4a05-a09d-595e42d2350f".parse().unwrap(),
          published_data::EMBEDDED_DB_META,
          published_data::EMBEDDED_DB_HEX,
        ),
      ],
    )
    .await;

  {
    let mut client_2 = TestClient::new_user().await;
    let workspace_id_2 = client_2.workspace_id().await;

    // Open workspace to trigger group creation
    client_2
      .open_collab(&workspace_id_2, &workspace_id_2, CollabType::Folder)
      .await;

    let fv = client_2
      .api_client
      .get_workspace_folder(&workspace_id_2, Some(5), None)
      .await
      .unwrap();

    // duplicate doc_with_embedded_db to workspace2
    // Result fv should be:
    // .
    // ├── Getting Started (existing)
    // └── db_with_embedded_db (inside should contain the database)
    client_2
      .api_client
      .duplicate_published_to_workspace(
        &workspace_id_2,
        &PublishedDuplicate {
          published_view_id: doc_with_embedded_db_view_id.to_string(),
          dest_view_id: fv.view_id, // use the root view
        },
      )
      .await
      .unwrap();

    {
      let fv = client_2
        .api_client
        .get_workspace_folder(&workspace_id_2, Some(5), None)
        .await
        .unwrap();
      let doc_with_embedded_db = fv
        .children
        .into_iter()
        .find(|v| v.name == "docwithembeddeddb")
        .unwrap();
      let doc_collab = client_2
        .get_collab_to_collab(
          workspace_id_2.clone(),
          doc_with_embedded_db.view_id.clone(),
          CollabType::Folder,
        )
        .await
        .unwrap();
      let doc = Document::open(doc_collab).unwrap();
      let doc_data = doc.get_document_data().unwrap();
      let grid = doc_data
        .blocks
        .iter()
        .find(|(_k, b)| b.ty == "grid")
        .unwrap()
        .1;

      // because it is embedded, the database parent's id is the view id of the doc
      let parent_id = grid.data.get("parent_id").unwrap().as_str().unwrap();
      assert_ne!(parent_id, doc_with_embedded_db.view_id.clone());
    }
  }
}

#[tokio::test]
async fn duplicate_to_workspace_db_with_relation() {
  let client_1 = TestClient::new_user().await;
  let workspace_id = client_1.workspace_id().await;

  // database with relation column to another database
  let db_with_rel_col_view_id: uuid::Uuid = uuid::Uuid::new_v4();

  client_1
    .publish_collabs(
      &workspace_id,
      vec![
        (
          db_with_rel_col_view_id,
          published_data::DB_WITH_REL_COL_META,
          published_data::DB_WITH_REL_COL_HEX,
        ),
        (
          // related database
          // uuid must be fixed because it is related to the db_with_rel_col
          "5fc669fa-8867-4f6d-98f1-ce387597eabd".parse().unwrap(),
          published_data::RELATED_DB_META,
          published_data::RELATED_DB_HEX,
        ),
      ],
    )
    .await;

  let db_with_row_doc_view_id: uuid::Uuid = uuid::Uuid::new_v4();
  client_1
    .publish_collabs(
      &workspace_id,
      vec![(
        db_with_row_doc_view_id,
        published_data::DB_ROW_WITH_DOC_META,
        published_data::DB_ROW_WITH_DOC_HEX,
      )],
    )
    .await;

  {
    let mut client_2 = TestClient::new_user().await;
    let workspace_id_2 = client_2.workspace_id().await;

    let fv = client_2
      .api_client
      .get_workspace_folder(&workspace_id_2, Some(5), None)
      .await
      .unwrap();

    // duplicate db_with_rel_col to workspace2
    // Result fv should be:
    // .
    // ├── Getting Started (existing)
    // ├── db_with_rel_col
    // └── related-db
    // related-db cannot be child of db_with_rel_col because they dont share the same field
    // and are 2 different databases, so we just put them in the root (dest_id)
    client_2
      .api_client
      .duplicate_published_to_workspace(
        &workspace_id_2,
        &PublishedDuplicate {
          published_view_id: db_with_rel_col_view_id.to_string(),
          dest_view_id: fv.view_id, // use the root view
        },
      )
      .await
      .unwrap();

    {
      let fv = client_2
        .api_client
        .get_workspace_folder(&workspace_id_2, Some(5), None)
        .await
        .unwrap();
      let db_with_rel_col = fv
        .children
        .iter()
        .find(|v| v.name == "grid3") // db_with_rel_col
        .unwrap();
      let related_db = fv
        .children
        .iter()
        .find(|v| v.name == "grid2") // related-db
        .unwrap();
      let db_with_rel_col_collab = client_2
        .get_db_collab_from_view(&workspace_id_2, &db_with_rel_col.view_id)
        .await;
      let related_db_collab = client_2
        .get_db_collab_from_view(&workspace_id_2, &related_db.view_id)
        .await;

      let related_db_id: String = related_db_collab
        .data
        .get_with_path(&related_db_collab.transact(), ["database", "id"])
        .unwrap();

      let rel_col_db_body = DatabaseBody::from_collab(&db_with_rel_col_collab).unwrap();
      let txn = db_with_rel_col_collab.transact();
      let all_fields = rel_col_db_body.fields.get_all_fields(&txn);
      all_fields
        .iter()
        .map(|f| &f.type_options)
        .flat_map(|t| t.iter())
        .filter(|(k, _v)| **k == FieldType::Relation.type_id())
        .map(|(_k, v)| v)
        .flat_map(|v| v.iter())
        .for_each(|(_k, db_id)| {
          assert_eq!(db_id.to_string(), related_db_id);
        });
    }
  }
}

#[tokio::test]
async fn duplicate_to_workspace_db_row_with_doc() {
  let client_1 = TestClient::new_user().await;
  let workspace_id = client_1.workspace_id().await;

  let db_with_row_doc_view_id: uuid::Uuid = uuid::Uuid::new_v4();
  client_1
    .publish_collabs(
      &workspace_id,
      vec![(
        db_with_row_doc_view_id,
        published_data::DB_ROW_WITH_DOC_META,
        published_data::DB_ROW_WITH_DOC_HEX,
      )],
    )
    .await;

  {
    let mut client_2 = TestClient::new_user().await;
    let workspace_id_2 = client_2.workspace_id().await;

    let fv = client_2
      .api_client
      .get_workspace_folder(&workspace_id_2, Some(5), None)
      .await
      .unwrap();

    // duplicate db_with_row_doc to workspace2
    // Result fv should be:
    // .
    // ├── Getting Started (existing)
    // └── db_with_row_doc
    client_2
      .api_client
      .duplicate_published_to_workspace(
        &workspace_id_2,
        &PublishedDuplicate {
          published_view_id: db_with_row_doc_view_id.to_string(),
          dest_view_id: fv.view_id, // use the root view
        },
      )
      .await
      .unwrap();

    {
      let fv = client_2
        .api_client
        .get_workspace_folder(&workspace_id_2, Some(5), None)
        .await
        .unwrap();
      let db_with_row_doc = fv
        .children
        .iter()
        .find(|v| v.name == "db_with_row_doc") // db_w ith_rel_col
        .unwrap();

      let db_collab = client_2
        .get_db_collab_from_view(&workspace_id_2, &db_with_row_doc.view_id)
        .await;

      let db_body = DatabaseBody::from_collab(&db_collab).unwrap();

      // check that doc exists and can be fetched
      let first_row_id = &db_body.views.get_all_views(&db_collab.transact())[0].row_orders[0].id;
      let row_collab = client_2
        .get_collab_to_collab(
          workspace_id_2.clone(),
          first_row_id.to_string(),
          CollabType::DatabaseRow,
        )
        .await
        .unwrap();
      let row_detail = RowDetail::from_collab(&row_collab).unwrap();
      assert!(!row_detail.meta.is_document_empty);
      let doc_id = row_detail.document_id;
      let _doc_collab = client_2
        .get_collab_to_collab(workspace_id_2.clone(), doc_id.clone(), CollabType::Document)
        .await;
      let folder_collab = client_2
        .get_collab_to_collab(workspace_id_2.clone(), workspace_id_2, CollabType::Folder)
        .await
        .unwrap();
      let folder = Folder::open(UserId::from(client_2.uid().await), folder_collab, None).unwrap();
      let doc_view = folder.get_view(&doc_id).unwrap();
      assert_eq!(doc_view.id, doc_view.parent_view_id);
    }
  }
}

#[tokio::test]
async fn duplicate_to_workspace_db_rel_self() {
  let client_1 = TestClient::new_user().await;
  let workspace_id = client_1.workspace_id().await;

  let db_rel_self_view_id: uuid::Uuid = "18d72589-80d7-4041-9342-5d572facb7c9".parse().unwrap();
  client_1
    .publish_collabs(
      &workspace_id,
      vec![(
        db_rel_self_view_id,
        published_data::DB_REL_SELF_META,
        published_data::DB_REL_SELF_HEX,
      )],
    )
    .await;

  {
    let mut client_2 = TestClient::new_user().await;
    let workspace_id_2 = client_2.workspace_id().await;
    let fv = client_2
      .api_client
      .get_workspace_folder(&workspace_id_2, Some(5), None)
      .await
      .unwrap();

    client_2
      .api_client
      .duplicate_published_to_workspace(
        &workspace_id_2,
        &PublishedDuplicate {
          published_view_id: db_rel_self_view_id.to_string(),
          dest_view_id: fv.view_id, // use the root view
        },
      )
      .await
      .unwrap();

    let fv = client_2
      .api_client
      .get_workspace_folder(&workspace_id_2, Some(5), None)
      .await
      .unwrap();
    println!("{:#?}", fv);

    let db_rel_self = fv
      .children
      .iter()
      .find(|v| v.name == "self_ref_db")
      .unwrap();

    let db_rel_self_collab = client_2
      .get_db_collab_from_view(&workspace_id_2, &db_rel_self.view_id)
      .await;
    let txn = db_rel_self_collab.transact();
    let db_rel_self_body = DatabaseBody::from_collab(&db_rel_self_collab).unwrap();
    let database_id = db_rel_self_body.get_database_id(&txn);
    let all_fields = db_rel_self_body.fields.get_all_fields(&txn);
    let rel_fields = all_fields
      .iter()
      .map(|f| &f.type_options)
      .flat_map(|t| t.iter())
      .filter(|(k, _v)| **k == FieldType::Relation.type_id())
      .map(|(_k, v)| v)
      .flat_map(|v| v.iter())
      .collect::<Vec<_>>();
    assert_eq!(rel_fields.len(), 1);
    assert_eq!(rel_fields[0].1.to_string(), database_id);
  }
}

#[tokio::test]
async fn duplicate_to_workspace_inline_db_doc_with_relation() {
  // scenario:
  // client_1 publish doc4
  // doc4 has inline database grid3
  // grid3 has relation to grid2
  // grid2 has relation to grid3
  // .
  // ├── grid2
  // └┬─ doc4
  //  └── grid3

  let client_1 = TestClient::new_user().await;
  let workspace_id = client_1.workspace_id().await;

  // database with a row referencing itself
  // uuid must be fixed because there is inline block that have parent_id that references this
  let doc_4_view_id: uuid::Uuid = "bb429fb8-3bb8-4f8c-99d8-0d8c48047efc".parse().unwrap();
  client_1
    .publish_collabs(
      &workspace_id,
      vec![
        (
          doc_4_view_id,
          published_data::DOC_4_META,
          published_data::DOC_4_DOC_STATE_HEX,
        ),
        (
          "5ecf6aa1-d4d6-47e4-af0f-3a7a9fd8299d".parse().unwrap(),
          published_data::GRID_3_META,
          published_data::GRID_3_HEX,
        ),
        (
          "cfdd03b2-b539-4f1d-84c3-3c0424bbd345".parse().unwrap(),
          published_data::GRID_2_META,
          published_data::GRID_2_HEX,
        ),
      ],
    )
    .await;

  {
    let client_2 = TestClient::new_user().await;
    let workspace_id_2 = client_2.workspace_id().await;
    let fv = client_2
      .api_client
      .get_workspace_folder(&workspace_id_2, Some(5), None)
      .await
      .unwrap();

    client_2
      .api_client
      .duplicate_published_to_workspace(
        &workspace_id_2,
        &PublishedDuplicate {
          published_view_id: doc_4_view_id.to_string(),
          dest_view_id: fv.view_id, // use the root view
        },
      )
      .await
      .unwrap();

    let fv = client_2
      .api_client
      .get_workspace_folder(&workspace_id_2, Some(5), None)
      .await
      .unwrap();

    let doc_4_fv = fv.children.iter().find(|v| v.name == "doc4").unwrap();
    let _ = doc_4_fv
      .children
      .iter()
      .find(|v| v.name == "grid3")
      .unwrap();
    let _ = fv.children.iter().find(|v| v.name == "grid2").unwrap();
  }
}

fn get_database_id_and_row_ids(published_db_blob: &[u8]) -> (String, HashSet<String>) {
  let pub_db_data = serde_json::from_slice::<PublishDatabaseData>(published_db_blob).unwrap();
  let db_collab = collab_from_doc_state(pub_db_data.database_collab, "").unwrap();
  let pub_db_id = DatabaseBody::database_id_from_collab(&db_collab).unwrap();
  let row_ids: HashSet<String> = pub_db_data.database_row_collabs.into_keys().collect();
  (pub_db_id, row_ids)
}
