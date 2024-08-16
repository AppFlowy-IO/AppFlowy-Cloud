use app_error::ErrorCode;
use client_api::entity::{
  AccountLink, PublishCollabItem, PublishCollabMetadata, TemplateCategoryType,
};
use client_api_test::*;
use collab::core::collab::DataSource;
use collab::core::origin::CollabOrigin;
use collab_document::document::Document;
use collab_entity::CollabType;
use database_entity::dto::{QueryCollab, QueryCollabParams};
use uuid::Uuid;

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
async fn get_user_default_workspace_test() {
  let email = generate_unique_email();
  let password = "Hello!123#";
  let c = localhost_client();
  c.sign_up(&email, password).await.unwrap();
  let mut test_client = TestClient::new_user().await;
  let workspace_id = test_client.workspace_id().await;
  let folder = test_client.get_user_folder().await;

  let views = folder.get_views_belong_to(&test_client.workspace_id().await);
  assert_eq!(views.len(), 1);
  assert_eq!(views[0].name, "Getting started");

  let document_id = views[0].id.clone();
  let document =
    get_document_collab_from_remote(&mut test_client, workspace_id, &document_id).await;
  let document_data = document.get_document_data().unwrap();
  assert_eq!(document_data.blocks.len(), 25);
}

async fn get_document_collab_from_remote(
  test_client: &mut TestClient,
  workspace_id: String,
  document_id: &str,
) -> Document {
  let params = QueryCollabParams {
    workspace_id,
    inner: QueryCollab {
      object_id: document_id.to_string(),
      collab_type: CollabType::Document,
    },
  };
  let resp = test_client.get_collab(params).await.unwrap();
  Document::from_doc_state(
    CollabOrigin::Empty,
    DataSource::DocStateV1(resp.encode_collab.doc_state.to_vec()),
    document_id,
    vec![],
  )
  .unwrap()
}

#[tokio::test]
async fn test_template_category_crud() {
  let (authorized_client, _) = generate_unique_registered_user_client().await;
  let category_name = Uuid::new_v4().to_string();
  let new_template_category = authorized_client
    .create_template_category(
      category_name.as_str(),
      "icon",
      "bg_color",
      "description",
      TemplateCategoryType::Feature,
      1,
    )
    .await
    .unwrap();
  assert_eq!(new_template_category.name, category_name);
  assert_eq!(new_template_category.icon, "icon");
  assert_eq!(new_template_category.bg_color, "bg_color");
  assert_eq!(new_template_category.description, "description");
  assert_eq!(
    new_template_category.category_type,
    TemplateCategoryType::Feature
  );
  assert_eq!(new_template_category.priority, 1);
  let updated_category_name = Uuid::new_v4().to_string();
  let updated_template_category = authorized_client
    .update_template_category(
      new_template_category.id,
      updated_category_name.as_str(),
      "new_icon",
      "new_bg_color",
      "new_description",
      TemplateCategoryType::UseCase,
      2,
    )
    .await
    .unwrap();
  assert_eq!(updated_template_category.name, updated_category_name);
  assert_eq!(updated_template_category.icon, "new_icon");
  assert_eq!(updated_template_category.bg_color, "new_bg_color");
  assert_eq!(updated_template_category.description, "new_description");
  assert_eq!(
    updated_template_category.category_type,
    TemplateCategoryType::UseCase
  );
  assert_eq!(updated_template_category.priority, 2);

  let guest_client = localhost_client();
  let template_category = guest_client
    .get_template_category(new_template_category.id)
    .await
    .unwrap();
  assert_eq!(template_category.name, updated_category_name);
  assert_eq!(template_category.icon, "new_icon");
  assert_eq!(template_category.bg_color, "new_bg_color");
  assert_eq!(template_category.description, "new_description");
  assert_eq!(
    template_category.category_type,
    TemplateCategoryType::UseCase
  );
  assert_eq!(template_category.priority, 2);

  let second_category_name = Uuid::new_v4().to_string();
  authorized_client
    .create_template_category(
      second_category_name.as_str(),
      "second_icon",
      "second_bg_color",
      "second_description",
      TemplateCategoryType::Feature,
      3,
    )
    .await
    .unwrap();
  let guest_client = localhost_client();
  let result = guest_client
    .create_template_category("", "", "", "", TemplateCategoryType::Feature, 0)
    .await;
  assert!(result.is_err());
  assert_eq!(result.unwrap_err().code, ErrorCode::NotLoggedIn);

  let name_search_substr = &second_category_name[0..second_category_name.len() - 1];
  let category_by_name_search_result = guest_client
    .get_template_categories(Some(name_search_substr), None)
    .await
    .unwrap()
    .categories;
  assert_eq!(category_by_name_search_result.len(), 1);
  assert_eq!(category_by_name_search_result[0].name, second_category_name);
  let category_by_type_search_result = guest_client
    .get_template_categories(None, Some(TemplateCategoryType::Feature))
    .await
    .unwrap()
    .categories;
  // Since the table might not be in a clean state, we can't guarantee that there is only one category of type Feature
  assert!(!category_by_type_search_result.is_empty());
  assert!(category_by_type_search_result
    .iter()
    .all(|r| r.category_type == TemplateCategoryType::Feature));
  assert!(category_by_type_search_result
    .iter()
    .any(|r| r.name == second_category_name));
  let result = guest_client
    .delete_template_category(new_template_category.id)
    .await;
  assert!(result.is_err());
  assert_eq!(result.unwrap_err().code, ErrorCode::NotLoggedIn);
  authorized_client
    .delete_template_category(new_template_category.id)
    .await
    .unwrap();
  let result = guest_client
    .get_template_category(new_template_category.id)
    .await;
  assert!(result.is_err());
  assert_eq!(result.unwrap_err().code, ErrorCode::RecordNotFound);
}

#[tokio::test]
async fn test_template_creator_crud() {
  let (authorized_client, _) = generate_unique_registered_user_client().await;
  let account_links = vec![AccountLink {
    link_type: "reddit".to_string(),
    url: "reddit_url".to_string(),
  }];
  let new_creator = authorized_client
    .create_template_creator("name", "avatar_url", account_links)
    .await
    .unwrap();
  assert_eq!(new_creator.name, "name");
  assert_eq!(new_creator.avatar_url, "avatar_url");
  assert_eq!(new_creator.account_links.len(), 1);
  assert_eq!(new_creator.account_links[0].link_type, "reddit");
  assert_eq!(new_creator.account_links[0].url, "reddit_url");

  let guest_client = localhost_client();
  let result = guest_client.create_template_creator("", "", vec![]).await;
  assert!(result.is_err());
  assert_eq!(result.unwrap_err().code, ErrorCode::NotLoggedIn);

  let updated_account_links = vec![AccountLink {
    link_type: "twitter".to_string(),
    url: "twitter_url".to_string(),
  }];
  let updated_creator = authorized_client
    .update_template_creator(
      new_creator.id,
      "new_name",
      "new_avatar_url",
      updated_account_links,
    )
    .await
    .unwrap();
  assert_eq!(updated_creator.name, "new_name");
  assert_eq!(updated_creator.avatar_url, "new_avatar_url");
  assert_eq!(updated_creator.account_links.len(), 1);
  assert_eq!(updated_creator.account_links[0].link_type, "twitter");
  assert_eq!(updated_creator.account_links[0].url, "twitter_url");

  let creator = guest_client
    .get_template_creator(new_creator.id)
    .await
    .unwrap();
  assert_eq!(creator.name, "new_name");
  assert_eq!(creator.avatar_url, "new_avatar_url");
  assert_eq!(creator.account_links.len(), 1);
  assert_eq!(creator.account_links[0].link_type, "twitter");
  assert_eq!(creator.account_links[0].url, "twitter_url");

  let result = guest_client.delete_template_creator(new_creator.id).await;
  assert!(result.is_err());
  assert_eq!(result.unwrap_err().code, ErrorCode::NotLoggedIn);
  authorized_client
    .delete_template_creator(new_creator.id)
    .await
    .unwrap();
  let result = guest_client.get_template_creator(new_creator.id).await;
  assert!(result.is_err());
  assert_eq!(result.unwrap_err().code, ErrorCode::RecordNotFound);
}

#[tokio::test]
async fn test_template_crud() {
  let (authorized_client, _) = generate_unique_registered_user_client().await;
  let workspace_id = get_first_workspace_string(&authorized_client).await;
  let published_view_namespace = uuid::Uuid::new_v4().to_string();
  authorized_client
    .set_workspace_publish_namespace(&workspace_id.to_string(), &published_view_namespace)
    .await
    .unwrap();
  let published_view_ids: Vec<Uuid> = (0..4).map(|_| Uuid::new_v4()).collect();
  let published_collab_items: Vec<PublishCollabItem<TemplateMetadata, &[u8]>> = published_view_ids
    .iter()
    .map(|view_id| PublishCollabItem {
      meta: PublishCollabMetadata {
        view_id: *view_id,
        publish_name: view_id.to_string(),
        metadata: TemplateMetadata {},
      },
      data: "yrs_encoded_data_1".as_bytes(),
    })
    .collect();

  authorized_client
    .publish_collabs::<TemplateMetadata, &[u8]>(&workspace_id, published_collab_items)
    .await
    .unwrap();

  let category_prefix = Uuid::new_v4().to_string();
  let category_1_name = format!("{}_1", category_prefix);
  let category_2_name = format!("{}_2", category_prefix);

  let creator = authorized_client
    .create_template_creator(
      "template_creator",
      "avatar_url",
      vec![AccountLink {
        link_type: "reddit".to_string(),
        url: "reddit_url".to_string(),
      }],
    )
    .await
    .unwrap();
  let creator_id = creator.id;
  let category_1_id = authorized_client
    .create_template_category(
      category_1_name.as_str(),
      "icon",
      "bg_color",
      "description",
      TemplateCategoryType::Feature,
      0,
    )
    .await
    .unwrap()
    .id;
  let category_2_id = authorized_client
    .create_template_category(
      category_2_name.as_str(),
      "icon",
      "bg_color",
      "description",
      TemplateCategoryType::Feature,
      0,
    )
    .await
    .unwrap()
    .id;

  let template_name_prefix = Uuid::new_v4().to_string();
  for (index, view_id) in published_view_ids[0..2].iter().enumerate() {
    let is_new_template = index % 2 == 0;
    let is_featured = true;
    let category_id = category_1_id;
    let template = authorized_client
      .create_template(
        *view_id,
        format!("{}-{}", template_name_prefix, view_id).as_str(),
        "description",
        "about",
        "view_url",
        vec![category_id],
        creator_id,
        is_new_template,
        is_featured,
        vec![],
      )
      .await
      .unwrap();
    assert_eq!(template.view_id, *view_id);
    assert!(template.categories.len() == 1);
    assert!(template.categories[0].id == category_id);
    assert!(template.creator.id == creator_id);
    assert!(template.creator.account_links.len() == 1);
    assert!(template.creator.account_links[0].url == creator.account_links[0].url);
    assert!(template.related_templates.len() == 0)
  }

  for (index, view_id) in published_view_ids[2..4].iter().enumerate() {
    let is_new_template = index % 2 == 0;
    let is_featured = false;
    let category_id = category_2_id;
    let template = authorized_client
      .create_template(
        *view_id,
        format!("template-{}", view_id).as_str(),
        "description",
        "about",
        "view_url",
        vec![category_id],
        creator_id,
        is_new_template,
        is_featured,
        vec![published_view_ids[0]],
      )
      .await
      .unwrap();
    assert_eq!(template.related_templates.len(), 1);
    assert_eq!(template.related_templates[0].view_id, published_view_ids[0]);
    assert_eq!(template.related_templates[0].categories.len(), 1);
    assert_eq!(
      template.related_templates[0].categories[0].id,
      category_1_id
    );
  }

  let guest_client = localhost_client();
  let templates = guest_client
    .get_templates(
      Some(category_1_id),
      None,
      None,
      Some(template_name_prefix.clone()),
    )
    .await
    .unwrap()
    .templates;
  assert_eq!(templates.len(), 2)
}

#[derive(serde::Serialize, serde::Deserialize)]
struct TemplateMetadata {}