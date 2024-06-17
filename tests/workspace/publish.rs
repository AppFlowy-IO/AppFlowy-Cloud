use client_api_test::{generate_unique_registered_user_client, localhost_client};

#[tokio::test]
async fn test_get_publish_namespace_not_set() {
  let (c, _user) = generate_unique_registered_user_client().await;
  let workspace_id = get_first_workspace_string(&c).await;
  let err = c
    .get_workspace_publish_namespace(&workspace_id.to_string())
    .await
    .err()
    .unwrap();

  assert_eq!(format!("{:?}", err.code), "PublishNamespaceNotSet");
}

#[tokio::test]
async fn test_set_publish_namespace_set() {
  let (c, _user) = generate_unique_registered_user_client().await;
  let workspace_id = get_first_workspace_string(&c).await;
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
  #[derive(serde::Serialize, serde::Deserialize)]
  struct Metadata {
    title: String,
  }

  let (c, _user) = generate_unique_registered_user_client().await;
  let workspace_id = get_first_workspace_string(&c).await;
  let my_namespace = uuid::Uuid::new_v4().to_string();
  c.set_workspace_publish_namespace(&workspace_id.to_string(), &my_namespace)
    .await
    .unwrap();

  c.publish_collab(
    &workspace_id,
    "my_doc",
    Metadata {
      title: "my_title".to_string(),
    },
  )
  .await
  .unwrap();

  {
    // Non login user should be able to view the published collab metadata
    let non_login = localhost_client();
    let published_collab = non_login
      .get_published_collab::<Metadata>(&my_namespace, "my_doc")
      .await
      .unwrap();
    assert_eq!(published_collab.title, "my_title");

    let collab_data = non_login
      .get_published_collab_blob(&my_namespace, "my_doc")
      .await
      .unwrap();
    assert!(collab_data.is_empty());
  }
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
