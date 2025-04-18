use crate::sql_test::util::{create_test_user, setup_db};
use database::chat::chat_ops::{
  delete_chat, get_all_chat_messages, insert_chat, insert_question_message, select_chat,
  select_chat_messages, select_chat_settings, update_chat_settings,
};
use serde_json::json;
use shared_entity::dto::chat_dto::{
  ChatAuthorType, ChatAuthorWithUuid, CreateChatParams, GetChatMessageParams,
};

use shared_entity::dto::chat_dto::UpdateChatParams;
use sqlx::PgPool;
use uuid::Uuid;

#[sqlx::test(migrations = false)]
async fn chat_crud_test(pool: PgPool) {
  setup_db(&pool).await.unwrap();

  let user_uuid = uuid::Uuid::new_v4();
  let name = user_uuid.to_string();
  let email = format!("{}@appflowy.io", name);
  let user = create_test_user(&pool, user_uuid, &email, &name)
    .await
    .unwrap();

  let chat_id = uuid::Uuid::new_v4().to_string();
  let rag_id_1 = Uuid::new_v4();
  let rag_id_2 = Uuid::new_v4();
  // create chat
  {
    insert_chat(
      &pool,
      &user.workspace_id,
      CreateChatParams {
        chat_id: chat_id.clone(),
        name: "my first chat".to_string(),
        rag_ids: vec![rag_id_1, rag_id_2],
      },
    )
    .await
    .unwrap();
  }

  // get chat
  {
    let chat = select_chat(&pool, &chat_id).await.unwrap();
    assert_eq!(chat.name, "my first chat");
    assert_eq!(
      chat.rag_ids,
      json!(vec![rag_id_1.to_string(), rag_id_2.to_string()]),
    );
  }

  // delete chat
  {
    let mut txn = pool.begin().await.unwrap();
    delete_chat(&mut txn, &chat_id).await.unwrap();
    txn.commit().await.unwrap();
  }

  // get chat
  {
    let result = select_chat(&pool, &chat_id).await.unwrap_err();
    assert!(result.is_record_not_found());
  }
}

#[sqlx::test(migrations = false)]
async fn chat_message_crud_test(pool: PgPool) {
  setup_db(&pool).await.unwrap();

  let user_uuid = uuid::Uuid::new_v4();
  let name = user_uuid.to_string();
  let email = format!("{}@appflowy.io", name);
  let user = create_test_user(&pool, user_uuid, &email, &name)
    .await
    .unwrap();

  let chat_id = uuid::Uuid::new_v4().to_string();
  let rag_id_1 = Uuid::new_v4();
  let rag_id_2 = Uuid::new_v4();
  // create chat
  {
    insert_chat(
      &pool,
      &user.workspace_id,
      CreateChatParams {
        chat_id: chat_id.clone(),
        name: "my first chat".to_string(),
        rag_ids: vec![rag_id_1, rag_id_2],
      },
    )
    .await
    .unwrap();
  }

  // create chat messages
  let uid = 0;
  let user_uuid = Uuid::new_v4();
  for i in 0..5 {
    let _ = insert_question_message(
      &pool,
      ChatAuthorWithUuid::new(uid, user_uuid, ChatAuthorType::System),
      &chat_id,
      format!("message {}", i),
    )
    .await
    .unwrap();
  }
  {
    let params = GetChatMessageParams::next_back(3);
    let mut txn = pool.begin().await.unwrap();
    let result = select_chat_messages(&mut txn, &chat_id, params)
      .await
      .unwrap();
    txn.commit().await.unwrap();
    assert_eq!(result.messages.len(), 3);
    assert_eq!(result.messages[0].message_id, 5);
    assert_eq!(result.messages[1].message_id, 4);
    assert_eq!(result.messages[2].message_id, 3);
    assert!(result.has_more);
  }

  // get 3 messages: 1,2,3
  {
    // option 1:use offset to get 3 messages => 1,2,3
    let mut txn = pool.begin().await.unwrap();
    let params = GetChatMessageParams::offset(0, 3);
    let result_1 = select_chat_messages(&mut txn, &chat_id, params)
      .await
      .unwrap();
    txn.commit().await.unwrap();
    assert_eq!(result_1.messages.len(), 3);
    assert_eq!(result_1.messages[0].message_id, 1);
    assert_eq!(result_1.messages[1].message_id, 2);
    assert_eq!(result_1.messages[2].message_id, 3);
    assert_eq!(result_1.total, 5);
    assert!(result_1.has_more);

    // option 2:use before_message_id to get 3 messages => 1,2,3
    let params = GetChatMessageParams::before_message_id(4, 3);
    let mut txn = pool.begin().await.unwrap();
    let result_2 = select_chat_messages(&mut txn, &chat_id, params)
      .await
      .unwrap();
    txn.commit().await.unwrap();
    assert_eq!(result_2.messages.len(), 3);
    assert_eq!(result_2.messages[0].message_id, 3);
    assert_eq!(result_2.messages[1].message_id, 2);
    assert_eq!(result_2.messages[2].message_id, 1);
    assert_eq!(result_2.total, 5);
    assert!(!result_2.has_more);
  }

  // get two messages: 4,5
  {
    // option 1:use offset to get 2 messages => 4,5
    let params = GetChatMessageParams::offset(3, 3);
    let mut txn = pool.begin().await.unwrap();
    let result_1 = select_chat_messages(&mut txn, &chat_id, params)
      .await
      .unwrap();
    txn.commit().await.unwrap();
    assert_eq!(result_1.messages.len(), 2);
    assert_eq!(result_1.messages[0].message_id, 4);
    assert_eq!(result_1.messages[1].message_id, 5);
    assert_eq!(result_1.total, 5);
    assert!(!result_1.has_more);

    // option 2:use after_message_id to get remaining 2 messages => 4,5
    let params = GetChatMessageParams::after_message_id(3, 3);
    let mut txn = pool.begin().await.unwrap();
    let result_2 = select_chat_messages(&mut txn, &chat_id, params)
      .await
      .unwrap();
    txn.commit().await.unwrap();
    assert_eq!(result_2.messages.len(), 2);
    assert_eq!(result_2.messages[0].message_id, 5);
    assert_eq!(result_2.messages[1].message_id, 4);
    assert_eq!(result_2.total, 5);
    assert!(!result_2.has_more);
  }

  // get all messages
  {
    let messages = get_all_chat_messages(&pool, &chat_id).await.unwrap();
    assert_eq!(messages.len(), 5);
  }
}

#[sqlx::test(migrations = false)]
async fn chat_setting_test(pool: PgPool) {
  setup_db(&pool).await.unwrap();
  let user_uuid = uuid::Uuid::new_v4();
  let name = user_uuid.to_string();
  let email = format!("{}@appflowy.io", name);
  let user = create_test_user(&pool, user_uuid, &email, &name)
    .await
    .unwrap();
  let workspace_id = user.workspace_id;
  let chat_id = uuid::Uuid::new_v4();
  let rag1 = Uuid::new_v4();
  let rag2 = Uuid::new_v4();

  // Insert initial chat data with rag_ids
  let insert_params = CreateChatParams {
    chat_id: chat_id.to_string(),
    name: "Initial Chat".to_string(),
    rag_ids: vec![rag1, rag2],
  };

  insert_chat(&pool, &workspace_id, insert_params)
    .await
    .expect("Failed to insert chat");

  // Validate inserted rag_ids
  let settings = select_chat_settings(&pool, &chat_id)
    .await
    .expect("Failed to get chat settings");
  assert_eq!(settings.rag_ids, vec![rag1.to_string(), rag2.to_string()]);

  // Update metadata
  let update_params = UpdateChatParams {
    name: None,
    metadata: Some(json!({"key": "value"})),
    rag_ids: None,
  };

  update_chat_settings(&pool, &chat_id, update_params)
    .await
    .expect("Failed to update chat settings");

  // Validate metadata update
  let settings = select_chat_settings(&pool, &chat_id)
    .await
    .expect("Failed to get chat settings");
  assert_eq!(settings.metadata, json!({"key": "value"}));

  // Update rag_ids and metadata together
  let rag3 = Uuid::new_v4();
  let rag4 = Uuid::new_v4();
  let update_params = UpdateChatParams {
    name: None,
    metadata: Some(json!({"new_key": "new_value"})),
    rag_ids: Some(vec![rag3.to_string(), rag4.to_string()]),
  };

  update_chat_settings(&pool, &chat_id, update_params)
    .await
    .expect("Failed to update chat settings");

  // Validate both rag_ids and metadata
  let settings = select_chat_settings(&pool, &chat_id)
    .await
    .expect("Failed to get chat settings");
  assert_eq!(
    settings.metadata,
    json!({"key": "value", "new_key": "new_value"})
  );
  assert_eq!(settings.rag_ids, vec![rag3.to_string(), rag4.to_string()]);
}
