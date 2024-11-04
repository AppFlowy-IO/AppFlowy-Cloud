use std::collections::HashSet;

use app_error::ErrorCode;
use client_api::entity::{
  AccountLink, CreateTemplateCategoryParams, CreateTemplateParams, PublishCollabItem,
  PublishCollabMetadata, TemplateCategoryType, UpdateTemplateCategoryParams, UpdateTemplateParams,
};
use client_api_test::*;
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
async fn test_template_category_crud() {
  let (authorized_client, _) = generate_unique_registered_user_client().await;
  let category_name = Uuid::new_v4().to_string();
  let params = CreateTemplateCategoryParams {
    name: category_name.clone(),
    icon: "icon".to_string(),
    bg_color: "bg_color".to_string(),
    description: "description".to_string(),
    category_type: TemplateCategoryType::Feature,
    priority: 1,
  };
  let new_template_category = authorized_client
    .create_template_category(&params)
    .await
    .unwrap();
  assert_eq!(new_template_category.name, category_name);
  assert_eq!(new_template_category.icon, params.icon);
  assert_eq!(new_template_category.bg_color, params.bg_color);
  assert_eq!(new_template_category.description, params.description);
  assert_eq!(
    new_template_category.category_type,
    TemplateCategoryType::Feature
  );
  assert_eq!(new_template_category.priority, 1);
  let updated_category_name = Uuid::new_v4().to_string();
  let params = UpdateTemplateCategoryParams {
    name: updated_category_name.clone(),
    icon: "new_icon".to_string(),
    bg_color: "new_bg_color".to_string(),
    description: "new_description".to_string(),
    category_type: TemplateCategoryType::UseCase,
    priority: 2,
  };
  let updated_template_category = authorized_client
    .update_template_category(new_template_category.id, &params)
    .await
    .unwrap();
  assert_eq!(updated_template_category.name, updated_category_name);
  assert_eq!(updated_template_category.icon, params.icon);
  assert_eq!(updated_template_category.bg_color, params.bg_color);
  assert_eq!(updated_template_category.description, params.description);
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
  assert_eq!(template_category.icon, params.icon);
  assert_eq!(template_category.bg_color, params.bg_color);
  assert_eq!(template_category.description, params.description);
  assert_eq!(
    template_category.category_type,
    TemplateCategoryType::UseCase
  );
  assert_eq!(template_category.priority, 2);

  let second_category_name = Uuid::new_v4().to_string();
  let params = CreateTemplateCategoryParams {
    name: second_category_name.clone(),
    icon: "second_icon".to_string(),
    bg_color: "second_bg_color".to_string(),
    description: "second_description".to_string(),
    category_type: TemplateCategoryType::Feature,
    priority: 3,
  };
  authorized_client
    .create_template_category(&params)
    .await
    .unwrap();
  let guest_client = localhost_client();
  let result = guest_client.create_template_category(&params).await;
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
  let creator_name_prefix = Uuid::new_v4().to_string();
  let creator_name = format!("{}-name", creator_name_prefix);
  let new_creator = authorized_client
    .create_template_creator(creator_name.as_str(), "avatar_url", account_links)
    .await
    .unwrap();
  assert_eq!(new_creator.name, creator_name);
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
  let updated_creator_name = format!("{}-new_name", creator_name_prefix);
  let updated_creator = authorized_client
    .update_template_creator(
      new_creator.id,
      updated_creator_name.as_str(),
      "new_avatar_url",
      updated_account_links,
    )
    .await
    .unwrap();
  assert_eq!(updated_creator.name, updated_creator_name);
  assert_eq!(updated_creator.avatar_url, "new_avatar_url");
  assert_eq!(updated_creator.account_links.len(), 1);
  assert_eq!(updated_creator.account_links[0].link_type, "twitter");
  assert_eq!(updated_creator.account_links[0].url, "twitter_url");

  let creator = guest_client
    .get_template_creator(new_creator.id)
    .await
    .unwrap();
  assert_eq!(creator.name, updated_creator_name);
  assert_eq!(creator.avatar_url, "new_avatar_url");
  assert_eq!(creator.account_links.len(), 1);
  assert_eq!(creator.account_links[0].link_type, "twitter");
  assert_eq!(creator.account_links[0].url, "twitter_url");

  let creators = guest_client
    .get_template_creators(Some(creator_name_prefix).as_deref())
    .await
    .unwrap()
    .creators;
  assert_eq!(creators.len(), 1);

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
    .set_workspace_publish_namespace(&workspace_id.to_string(), published_view_namespace.clone())
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

  let creator_1 = authorized_client
    .create_template_creator(
      "template_creator 1",
      "avatar_url",
      vec![AccountLink {
        link_type: "reddit".to_string(),
        url: "reddit_url".to_string(),
      }],
    )
    .await
    .unwrap();
  let creator_2 = authorized_client
    .create_template_creator(
      "template_creator 2",
      "avatar_url",
      vec![AccountLink {
        link_type: "facebook".to_string(),
        url: "facebook_url".to_string(),
      }],
    )
    .await
    .unwrap();
  let params = CreateTemplateCategoryParams {
    name: category_1_name,
    icon: "icon".to_string(),
    bg_color: "bg_color".to_string(),
    description: "description".to_string(),
    category_type: TemplateCategoryType::Feature,
    priority: 0,
  };
  let category_1 = authorized_client
    .create_template_category(&params)
    .await
    .unwrap();

  let params = CreateTemplateCategoryParams {
    name: category_2_name,
    icon: "icon".to_string(),
    bg_color: "bg_color".to_string(),
    description: "description".to_string(),
    category_type: TemplateCategoryType::Feature,
    priority: 0,
  };
  let category_2 = authorized_client
    .create_template_category(&params)
    .await
    .unwrap();

  let template_name_prefix = Uuid::new_v4().to_string();
  for (index, view_id) in published_view_ids[0..2].iter().enumerate() {
    let is_new_template = index % 2 == 0;
    let is_featured = true;
    let category_id = category_1.id;
    let params = CreateTemplateParams {
      view_id: *view_id,
      name: format!("{}-{}", template_name_prefix, view_id),
      description: "description".to_string(),
      about: "about".to_string(),
      view_url: "view_url".to_string(),
      category_ids: vec![category_id],
      creator_id: creator_1.id,
      is_new_template,
      is_featured,
      related_view_ids: vec![],
    };
    let template = authorized_client.create_template(&params).await.unwrap();
    assert_eq!(template.view_id, *view_id);
    assert_eq!(template.categories.len(), 1);
    assert_eq!(template.categories[0].id, category_id);
    assert_eq!(template.creator.id, creator_1.id);
    assert_eq!(template.creator.name, creator_1.name);
    assert_eq!(template.creator.account_links.len(), 1);
    assert_eq!(
      template.creator.account_links[0].url,
      creator_1.account_links[0].url
    );
    assert!(template.related_templates.is_empty())
  }

  for (index, view_id) in published_view_ids[2..4].iter().enumerate() {
    let is_new_template = index % 2 == 0;
    let is_featured = false;
    let category_id = category_2.id;
    let params = CreateTemplateParams {
      view_id: *view_id,
      name: format!("{}-{}", template_name_prefix, view_id),
      description: "description".to_string(),
      about: "about".to_string(),
      view_url: "view_url".to_string(),
      category_ids: vec![category_id],
      creator_id: creator_2.id,
      is_new_template,
      is_featured,
      related_view_ids: vec![published_view_ids[0]],
    };
    let template = authorized_client.create_template(&params).await.unwrap();
    assert_eq!(template.related_templates.len(), 1);
    assert_eq!(template.related_templates[0].view_id, published_view_ids[0]);
    assert_eq!(template.related_templates[0].creator.id, creator_1.id);
    assert_eq!(template.related_templates[0].categories.len(), 1);
    assert_eq!(
      template.related_templates[0].categories[0].id,
      category_1.id
    );
  }

  let guest_client = localhost_client();
  let templates = guest_client
    .get_templates(
      Some(category_2.id),
      None,
      None,
      Some(template_name_prefix.clone()),
    )
    .await
    .unwrap()
    .templates;
  let view_ids: HashSet<Uuid> = templates.iter().map(|t| t.template.view_id).collect();
  assert_eq!(templates.len(), 2);
  assert!(view_ids.contains(&published_view_ids[2]));
  assert!(view_ids.contains(&published_view_ids[3]));
  assert_eq!(
    templates[0].publish_info.namespace,
    published_view_namespace
  );

  let featured_templates = guest_client
    .get_templates(None, Some(true), None, Some(template_name_prefix.clone()))
    .await
    .unwrap()
    .templates;
  let featured_view_ids: HashSet<Uuid> = featured_templates
    .iter()
    .map(|t| t.template.view_id)
    .collect();
  assert_eq!(featured_templates.len(), 2);
  assert!(featured_view_ids.contains(&published_view_ids[0]));
  assert!(featured_view_ids.contains(&published_view_ids[1]));

  let new_templates = guest_client
    .get_templates(None, None, Some(true), Some(template_name_prefix.clone()))
    .await
    .unwrap()
    .templates;
  let new_view_ids: HashSet<Uuid> = new_templates.iter().map(|t| t.template.view_id).collect();
  assert_eq!(new_templates.len(), 2);
  assert!(new_view_ids.contains(&published_view_ids[0]));
  assert!(new_view_ids.contains(&published_view_ids[2]));

  let template = guest_client
    .get_template(published_view_ids[3])
    .await
    .unwrap();
  assert_eq!(template.template.view_id, published_view_ids[3]);
  assert_eq!(template.template.creator.id, creator_2.id);
  assert_eq!(template.template.categories.len(), 1);
  assert_eq!(template.template.categories[0].id, category_2.id);
  assert_eq!(template.template.related_templates.len(), 1);
  assert_eq!(
    template.template.related_templates[0].view_id,
    published_view_ids[0]
  );
  assert_eq!(
    template.publish_info.namespace,
    published_view_namespace.clone()
  );

  let params = UpdateTemplateParams {
    name: format!("{}-{}", template_name_prefix, published_view_ids[3]),
    description: "description".to_string(),
    about: "about".to_string(),
    view_url: "view_url".to_string(),
    category_ids: vec![category_1.id],
    creator_id: creator_2.id,
    is_new_template: false,
    is_featured: true,
    related_view_ids: vec![published_view_ids[0]],
  };
  authorized_client
    .update_template(published_view_ids[3], &params)
    .await
    .unwrap();

  authorized_client
    .delete_template(published_view_ids[3])
    .await
    .unwrap();
  let resp = guest_client.get_template(published_view_ids[3]).await;
  assert!(resp.is_err());
  assert_eq!(resp.unwrap_err().code, ErrorCode::RecordNotFound);
}

#[derive(serde::Serialize, serde::Deserialize)]
struct TemplateMetadata {}
