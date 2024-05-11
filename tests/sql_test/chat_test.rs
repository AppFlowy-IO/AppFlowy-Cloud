use crate::sql_test::util::{setup_db, test_create_user};
use database::chat::chat_ops::{
  delete_chat, get_all_chat_messages, get_chat, insert_chat, insert_chat_message,
  select_chat_messages,
};
use database_entity::chat::{CreateChatMessageParams, CreateChatParams, GetChatMessageParams};
use serde_json::json;
use sqlx::PgPool;

#[sqlx::test(migrations = false)]
async fn chat_crud_test(pool: PgPool) {
  setup_db(&pool).await.unwrap();

  let user_uuid = uuid::Uuid::new_v4();
  let name = user_uuid.to_string();
  let email = format!("{}@appflowy.io", name);
  let user = test_create_user(&pool, user_uuid, &email, &name)
    .await
    .unwrap();

  let chat_id = uuid::Uuid::new_v4().to_string();
  // create chat
  {
    let mut txn = pool.begin().await.unwrap();
    insert_chat(
      &mut txn,
      &user.workspace_id,
      CreateChatParams {
        chat_id: chat_id.clone(),
        name: "my first chat".to_string(),
        rag_ids: vec!["rag_id_1".to_string(), "rag_id_2".to_string()],
      },
    )
    .await
    .unwrap();
    txn.commit().await.unwrap();
  }

  // get chat
  {
    let chat = get_chat(&pool, &chat_id).await.unwrap();
    assert_eq!(chat.name, "my first chat");
    assert_eq!(
      chat.rag_ids,
      json!(vec!["rag_id_1".to_string(), "rag_id_2".to_string()]),
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
    let result = get_chat(&pool, &chat_id).await.unwrap_err();
    assert!(result.is_record_not_found());
  }
}

#[sqlx::test(migrations = false)]
async fn chat_message_crud_test(pool: PgPool) {
  setup_db(&pool).await.unwrap();

  let user_uuid = uuid::Uuid::new_v4();
  let name = user_uuid.to_string();
  let email = format!("{}@appflowy.io", name);
  let user = test_create_user(&pool, user_uuid, &email, &name)
    .await
    .unwrap();

  let chat_id = uuid::Uuid::new_v4().to_string();
  // create chat
  {
    let mut txn = pool.begin().await.unwrap();
    insert_chat(
      &mut txn,
      &user.workspace_id,
      CreateChatParams {
        chat_id: chat_id.clone(),
        name: "my first chat".to_string(),
        rag_ids: vec!["rag_id_1".to_string(), "rag_id_2".to_string()],
      },
    )
    .await
    .unwrap();
    txn.commit().await.unwrap();
  }

  // create chat messages
  for i in 0..5 {
    let params = CreateChatMessageParams {
      content: format!("message {}", i),
    };
    insert_chat_message(&pool, &chat_id, params).await.unwrap();
  }

  // get 3 messages
  {
    let mut txn = pool.begin().await.unwrap();
    let params = GetChatMessageParams {
      chat_id: chat_id.clone(),
      limit: 3,
      offset: 0,
      after_message_id: None,
    };
    let result = select_chat_messages(&mut txn, &chat_id, params)
      .await
      .unwrap();
    txn.commit().await.unwrap();
    assert_eq!(result.messages.len(), 3);
    assert_eq!(result.total, 5);
    assert!(result.has_more);
  }

  // get remaining 2 messages
  {
    let params = GetChatMessageParams {
      chat_id: chat_id.clone(),
      limit: 3,
      offset: 3,
      after_message_id: None,
    };

    let mut txn = pool.begin().await.unwrap();
    let result = select_chat_messages(&mut txn, &chat_id, params)
      .await
      .unwrap();
    txn.commit().await.unwrap();
    assert_eq!(result.messages.len(), 2);
    assert_eq!(result.total, 5);
    assert!(!result.has_more);
  }

  // get all messages at once
  {
    let messages = get_all_chat_messages(&pool, &chat_id).await.unwrap();
    assert_eq!(messages.len(), 5);
  }
}
