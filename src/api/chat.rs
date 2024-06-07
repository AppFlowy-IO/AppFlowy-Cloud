use crate::biz::chat::ops::{
  create_chat, create_chat_message, create_chat_question, delete_chat,
  generate_chat_message_answer, get_chat_messages, update_chat_message,
};
use crate::state::AppState;
use actix_web::web::{Data, Json};
use actix_web::{web, HttpResponse, Scope};

use app_error::AppError;
use appflowy_ai_client::dto::RepeatedRelatedQuestion;
use authentication::jwt::UserUuid;
use bytes::Bytes;
use database_entity::dto::{
  ChatAuthor, ChatMessage, CreateAnswerMessageParams, CreateChatMessageParams, CreateChatParams,
  GetChatMessageParams, MessageCursor, RepeatedChatMessage, UpdateChatMessageContentParams,
};
use futures::Stream;
use futures_util::{FutureExt, TryStreamExt};
use pin_project::pin_project;
use shared_entity::response::{AppResponse, JsonAppResponse};
use std::collections::HashMap;
use std::pin::Pin;

use std::task::{Context, Poll};
use tokio::sync::oneshot;
use tokio::task;

use database::chat;

use database::chat::chat_ops::insert_answer_message;
use tracing::{error, instrument, trace};
use validator::Validate;

pub fn chat_scope() -> Scope {
  web::scope("/api/chat/{workspace_id}")
    .service(web::resource("").route(web::post().to(create_chat_handler)))
    .service(
      web::resource("/{chat_id}")
        .route(web::delete().to(delete_chat_handler))
        .route(web::get().to(get_chat_message_handler)),
    )
    .service(
      web::resource("/{chat_id}/{message_id}/related_question")
        .route(web::get().to(get_related_message_handler)),
    )
    .service(
      web::resource("/{chat_id}/message")
        .route(web::post().to(create_chat_message_handler))
        .route(web::put().to(update_chat_message_handler)),
    )
    .service(
      // Create a question for given chat
      web::resource("/{chat_id}/message/question").route(web::post().to(create_question_handler)),
    )
      // create a answer for given chat
    .service(web::resource("/{chat_id}/message/answer").route(web::post().to(create_answer_handler)))
    .service(
      // Generate answer for given question.
      web::resource("/{chat_id}/{message_id}/answer").route(web::get().to(gen_answer_handler)),
    )
      // Stream the answer for given question.
    .service(
      web::resource("/{chat_id}/{message_id}/answer/stream")
        .route(web::get().to(answer_stream_handler)),
    )
}
async fn create_chat_handler(
  path: web::Path<String>,
  state: Data<AppState>,
  payload: Json<CreateChatParams>,
) -> actix_web::Result<JsonAppResponse<()>> {
  let workspace_id = path.into_inner();
  let params = payload.into_inner();
  trace!("create new chat: {:?}", params);
  create_chat(&state.pg_pool, params, &workspace_id).await?;
  Ok(AppResponse::Ok().into())
}

async fn delete_chat_handler(
  path: web::Path<(String, String)>,
  state: Data<AppState>,
) -> actix_web::Result<JsonAppResponse<()>> {
  let (_workspace_id, chat_id) = path.into_inner();
  delete_chat(&state.pg_pool, &chat_id).await?;
  Ok(AppResponse::Ok().into())
}

#[instrument(level = "info", skip_all, err)]
async fn create_chat_message_handler(
  state: Data<AppState>,
  path: web::Path<(String, String)>,
  payload: Json<CreateChatMessageParams>,
  uuid: UserUuid,
) -> actix_web::Result<HttpResponse> {
  let (_workspace_id, chat_id) = path.into_inner();
  let params = payload.into_inner();

  if let Err(err) = params.validate() {
    return Ok(HttpResponse::from_error(AppError::from(err)));
  }

  let uid = state.user_cache.get_user_uid(&uuid).await?;
  let message_stream = create_chat_message(
    &state.pg_pool,
    uid,
    chat_id,
    params,
    state.ai_client.clone(),
  )
  .await;

  Ok(
    HttpResponse::Ok()
      .content_type("application/json")
      .streaming(message_stream),
  )
}

async fn update_chat_message_handler(
  state: Data<AppState>,
  payload: Json<UpdateChatMessageContentParams>,
) -> actix_web::Result<JsonAppResponse<()>> {
  let params = payload.into_inner();
  update_chat_message(&state.pg_pool, params, state.ai_client.clone()).await?;
  Ok(AppResponse::Ok().into())
}

async fn get_related_message_handler(
  path: web::Path<(String, String, i64)>,
  state: Data<AppState>,
) -> actix_web::Result<JsonAppResponse<RepeatedRelatedQuestion>> {
  let (_workspace_id, chat_id, message_id) = path.into_inner();
  let resp = state
    .ai_client
    .get_related_question(&chat_id, &message_id)
    .await
    .map_err(|err| AppError::Internal(err.into()))?;
  Ok(AppResponse::Ok().with_data(resp).into())
}

async fn create_question_handler(
  state: Data<AppState>,
  path: web::Path<(String, String)>,
  payload: Json<CreateChatMessageParams>,
  uuid: UserUuid,
) -> actix_web::Result<JsonAppResponse<ChatMessage>> {
  let (_workspace_id, chat_id) = path.into_inner();
  let params = payload.into_inner();

  let uid = state.user_cache.get_user_uid(&uuid).await?;
  let resp = create_chat_question(&state.pg_pool, uid, chat_id, params).await?;
  Ok(AppResponse::Ok().with_data(resp).into())
}

async fn create_answer_handler(
  path: web::Path<(String, String)>,
  payload: Json<CreateAnswerMessageParams>,
  state: Data<AppState>,
) -> actix_web::Result<JsonAppResponse<ChatMessage>> {
  let payload = payload.into_inner();
  payload.validate().map_err(AppError::from)?;

  let (_workspace_id, chat_id) = path.into_inner();
  let message = insert_answer_message(
    &state.pg_pool,
    ChatAuthor::ai(),
    &chat_id,
    payload.content,
    payload.question_message_id,
  )
  .await?;

  Ok(AppResponse::Ok().with_data(message).into())
}
async fn gen_answer_handler(
  path: web::Path<(String, String, i64)>,
  state: Data<AppState>,
) -> actix_web::Result<JsonAppResponse<ChatMessage>> {
  let (_workspace_id, chat_id, message_id) = path.into_inner();
  let message = generate_chat_message_answer(
    &state.pg_pool,
    state.ai_client.clone(),
    message_id,
    &chat_id,
  )
  .await?;
  Ok(AppResponse::Ok().with_data(message).into())
}

#[instrument(level = "info", skip_all, err)]
async fn answer_stream_handler(
  path: web::Path<(String, String, i64)>,
  state: Data<AppState>,
) -> actix_web::Result<HttpResponse> {
  let (_workspace_id, chat_id, question_id) = path.into_inner();
  let content = chat::chat_ops::select_chat_message_content(&state.pg_pool, question_id).await?;
  let answer_stream = state
    .ai_client
    .stream_question(&chat_id, &content)
    .await
    .map_err(|err| AppError::Internal(err.into()))?;

  let finish_action = move |collected_bytes: Vec<u8>| {
    task::spawn(async move {
      if let Ok(final_message) = String::from_utf8(collected_bytes) {
        match chat::chat_ops::insert_answer_message(
          &state.pg_pool,
          ChatAuthor::ai(),
          &chat_id,
          final_message,
          question_id,
        )
        .await
        {
          Ok(message) => {
            let json_bytes = serde_json::to_vec(&message)?;
            Ok(Bytes::from(json_bytes))
          },
          Err(err) => {
            error!("Failed to insert answer message: {}", err);
            Err(AppError::Internal(err.into()))
          },
        }
      } else {
        error!("Stream finished with invalid UTF-8 data.");
        Err(AppError::InvalidRequest("Invalid UTF-8 data".to_string()))
      }
    })
  };

  let new_answer_stream = answer_stream.map_err(AppError::from);
  let finish_answer_stream = CollectingStream::new(new_answer_stream, finish_action);
  Ok(
    HttpResponse::Ok()
      .content_type("text/event-stream")
      .streaming(finish_answer_stream),
  )
}

#[instrument(level = "debug", skip_all, err)]
async fn get_chat_message_handler(
  path: web::Path<(String, String)>,
  query: web::Query<HashMap<String, String>>,
  state: Data<AppState>,
) -> actix_web::Result<JsonAppResponse<RepeatedChatMessage>> {
  let mut params = GetChatMessageParams {
    cursor: MessageCursor::Offset(0),
    limit: query
      .get("limit")
      .and_then(|s| s.parse::<u64>().ok())
      .unwrap_or(10),
  };
  if let Some(value) = query.get("offset").and_then(|s| s.parse::<u64>().ok()) {
    params.cursor = MessageCursor::Offset(value);
  } else if let Some(value) = query.get("after").and_then(|s| s.parse::<i64>().ok()) {
    params.cursor = MessageCursor::AfterMessageId(value);
  } else if let Some(value) = query.get("before").and_then(|s| s.parse::<i64>().ok()) {
    params.cursor = MessageCursor::BeforeMessageId(value);
  } else {
    params.cursor = MessageCursor::NextBack;
  }

  trace!("get chat messages: {:?}", params);
  let (_workspace_id, chat_id) = path.into_inner();
  let messages = get_chat_messages(&state.pg_pool, params, &chat_id).await?;
  Ok(AppResponse::Ok().with_data(messages).into())
}

#[pin_project]
pub struct FinalAnswerStream<S, F> {
  #[pin]
  stream: S,
  buffer: Vec<u8>,
  action: Option<F>,
}

impl<S, F> FinalAnswerStream<S, F> {
  pub fn new(stream: S, action: F) -> Self {
    FinalAnswerStream {
      stream,
      buffer: Vec::new(),
      action: Some(action),
    }
  }
}

impl<S, F> Stream for FinalAnswerStream<S, F>
where
  S: Stream<Item = Result<String, AppError>>,
  F: FnOnce(Vec<u8>),
{
  type Item = Result<Bytes, AppError>;

  fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
    let this = self.project();

    match this.stream.poll_next(cx) {
      Poll::Ready(Some(Ok(item))) => {
        let bytes = item.into_bytes();
        this.buffer.extend_from_slice(&bytes);
        Poll::Ready(Some(Ok(Bytes::from(bytes))))
      },
      Poll::Ready(Some(Err(err))) => Poll::Ready(Some(Err(err))),
      Poll::Ready(None) => {
        if let Some(action) = this.action.take() {
          action(std::mem::take(this.buffer));
        }
        Poll::Ready(None)
      },
      Poll::Pending => Poll::Pending,
    }
  }
}

#[pin_project]
pub struct CollectingStream<S, F> {
  #[pin]
  stream: S,
  buffer: Vec<u8>,
  action: Option<F>,
  state: CollectingStreamType,
  result_receiver: Option<oneshot::Receiver<Result<Bytes, AppError>>>,
}

enum CollectingStreamType {
  AnswerString,
  AnswerMessage,
}

impl<S, F> CollectingStream<S, F> {
  pub fn new(stream: S, action: F) -> Self {
    CollectingStream {
      stream,
      buffer: Vec::new(),
      action: Some(action),
      state: CollectingStreamType::AnswerString,
      result_receiver: None,
    }
  }
}

impl<S, F> Stream for CollectingStream<S, F>
where
  S: Stream<Item = Result<Bytes, AppError>>,
  F: FnOnce(Vec<u8>) -> task::JoinHandle<Result<Bytes, AppError>> + Send + 'static,
{
  type Item = Result<Bytes, AppError>;

  fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
    let mut this = self.project();
    match this.state {
      CollectingStreamType::AnswerString => {
        match this.stream.as_mut().poll_next(cx) {
          Poll::Ready(Some(Ok(bytes))) => {
            if let Some(&b'\n') = bytes.last() {
              this.buffer.extend_from_slice(&bytes[..bytes.len() - 1]);
            } else {
              this.buffer.extend_from_slice(&bytes);
            }
            Poll::Ready(Some(Ok(bytes)))
          },
          Poll::Ready(Some(Err(err))) => Poll::Ready(Some(Err(err))),
          Poll::Ready(None) => {
            if let Some(action) = this.action.take() {
              let buffer = std::mem::take(this.buffer);
              let (sender, receiver) = oneshot::channel();
              *this.result_receiver = Some(receiver);
              *this.state = CollectingStreamType::AnswerMessage;

              // Spawn the async task to handle the buffer and send the result
              tokio::spawn(async move {
                let result = action(buffer).await;
                sender.send(result.unwrap()).unwrap();
              });

              Poll::Ready(Some(Ok(Bytes::from(""))))
            } else {
              // If action is None, it means the stream is finished.
              Poll::Ready(None)
            }
          },
          Poll::Pending => Poll::Pending,
        }
      },
      CollectingStreamType::AnswerMessage => {
        if let Some(receiver) = this.result_receiver.as_mut() {
          match receiver.poll_unpin(cx) {
            Poll::Ready(Ok(result)) => {
              this.result_receiver.take();
              Poll::Ready(Some(result))
            },
            Poll::Ready(Err(_)) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
          }
        } else {
          Poll::Ready(None)
        }
      },
    }
  }
}
