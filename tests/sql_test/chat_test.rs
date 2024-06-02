use crate::sql_test::util::{setup_db, test_create_user};
use database::chat::chat_ops::{
  delete_chat, get_all_chat_messages, insert_chat, insert_question_message, select_chat,
  select_chat_messages,
};
use database_entity::dto::{ChatAuthor, ChatAuthorType, CreateChatParams, GetChatMessageParams};
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
    let chat = select_chat(&pool, &chat_id).await.unwrap();
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
    let _ = insert_question_message(
      &pool,
      ChatAuthor::new(0, ChatAuthorType::System),
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
    assert_eq!(result.messages[0].message_id, 3);
    assert_eq!(result.messages[1].message_id, 4);
    assert_eq!(result.messages[2].message_id, 5);
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
    assert_eq!(result_2.messages[0].message_id, 1);
    assert_eq!(result_2.messages[1].message_id, 2);
    assert_eq!(result_2.messages[2].message_id, 3);
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
    assert_eq!(result_2.messages[0].message_id, 4);
    assert_eq!(result_2.messages[1].message_id, 5);
    assert_eq!(result_2.total, 5);
    assert!(!result_2.has_more);
  }

  // get all messages
  {
    let messages = get_all_chat_messages(&pool, &chat_id).await.unwrap();
    assert_eq!(messages.len(), 5);
  }
}
