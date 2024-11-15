use crate::biz::chat::ops::{
  create_chat, create_chat_message, delete_chat, generate_chat_message_answer, get_chat_messages,
  update_chat_message,
};
use crate::state::AppState;
use actix_web::web::{Data, Json};
use actix_web::{web, HttpRequest, HttpResponse, Scope};

use app_error::AppError;
use appflowy_ai_client::dto::{CreateChatContext, RepeatedRelatedQuestion};
use authentication::jwt::UserUuid;
use bytes::Bytes;
use futures::Stream;
use futures_util::stream;
use futures_util::{FutureExt, TryStreamExt};
use pin_project::pin_project;
use shared_entity::dto::chat_dto::{
  ChatAuthor, ChatMessage, CreateAnswerMessageParams, CreateChatMessageParams,
  CreateChatMessageParamsV2, CreateChatParams, GetChatMessageParams, MessageCursor,
  RepeatedChatMessage, UpdateChatMessageContentParams,
};
use shared_entity::response::{AppResponse, JsonAppResponse};
use std::collections::HashMap;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::sync::oneshot;
use tokio::task;

use database::chat;

use crate::api::util::ai_model_from_header;

use database::chat::chat_ops::insert_answer_message;
use tracing::{error, instrument, trace};
use validator::Validate;
pub fn chat_scope() -> Scope {
  web::scope("/api/chat/{workspace_id}")
      // Chat management
      .service(
        web::resource("")
            .route(web::post().to(create_chat_handler))
      )
      .service(
        web::resource("/{chat_id}")
            .route(web::delete().to(delete_chat_handler))
            .route(web::get().to(get_chat_message_handler))
      )

      // Message management
      .service(
        web::resource("/{chat_id}/message")
            .route(web::put().to(update_question_handler))
      )
      .service(
        web::resource("/{chat_id}/message/question")
            .route(web::post().to(create_question_handler))
      )
      .service(
        web::resource("/{chat_id}/v2/message/question")
            .route(web::post().to(create_question_handler_v2))
      )
      .service(
        web::resource("/{chat_id}/message/answer")
            .route(web::post().to(save_answer_handler))
      )

      // AI response generation
      .service(
        web::resource("/{chat_id}/{message_id}/answer")
            .route(web::get().to(answer_handler))
      )
      .service(
        web::resource("/{chat_id}/{message_id}/answer/stream")
            .route(web::get().to(answer_stream_handler)) // Deprecated
      )
      .service(
        web::resource("/{chat_id}/{message_id}/v2/answer/stream")
            .route(web::get().to(answer_stream_v2_handler))
      )

      // Additional functionality
      .service(
        web::resource("/{chat_id}/{message_id}/related_question")
            .route(web::get().to(get_related_message_handler))
      )
      .service(
        web::resource("/{chat_id}/context/text")
            .route(web::post().to(create_chat_context_handler))
      )
}
async fn create_chat_handler(
  path: web::Path<String>,
  state: Data<AppState>,
  payload: Json<CreateChatParams>,
) -> actix_web::Result<JsonAppResponse<()>> {
  let workspace_id = path.into_inner();
  let params = payload.into_inner();
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

#[instrument(level = "debug", skip_all, err)]
async fn create_chat_context_handler(
  state: Data<AppState>,
  payload: Json<CreateChatContext>,
) -> actix_web::Result<JsonAppResponse<()>> {
  let params = payload.into_inner();
  state
    .ai_client
    .create_chat_text_context(params)
    .await
    .map_err(AppError::from)?;
  Ok(AppResponse::Ok().into())
}

async fn update_question_handler(
  state: Data<AppState>,
  payload: Json<UpdateChatMessageContentParams>,
  req: HttpRequest,
) -> actix_web::Result<JsonAppResponse<()>> {
  let params = payload.into_inner();
  let ai_model = ai_model_from_header(&req);
  update_chat_message(&state.pg_pool, params, state.ai_client.clone(), ai_model).await?;
  Ok(AppResponse::Ok().into())
}

async fn get_related_message_handler(
  path: web::Path<(String, String, i64)>,
  state: Data<AppState>,
  req: HttpRequest,
) -> actix_web::Result<JsonAppResponse<RepeatedRelatedQuestion>> {
  let (_workspace_id, chat_id, message_id) = path.into_inner();
  let ai_model = ai_model_from_header(&req);
  let resp = state
    .ai_client
    .get_related_question(&chat_id, &message_id, &ai_model)
    .await
    .map_err(|err| AppError::Internal(err.into()))?;
  Ok(AppResponse::Ok().with_data(resp).into())
}

#[instrument(level = "debug", skip_all, err)]
async fn create_question_handler(
  state: Data<AppState>,
  path: web::Path<(String, String)>,
  payload: Json<CreateChatMessageParams>,
  uuid: UserUuid,
) -> actix_web::Result<JsonAppResponse<ChatMessage>> {
  let (_workspace_id, chat_id) = path.into_inner();
  let params = payload.into_inner();

  // When create a question, we will extract the metadata from the question content.
  // metadata might include user mention file,page,or user. For example, @Get started.
  for metadata in params.metadata.clone() {
    let (data, desc) = metadata.split_data();
    if let Err(err) = data.validate() {
      error!("Failed to validate metadata: {}", err);
      continue;
    }

    let context =
      CreateChatContext::new(chat_id.clone(), data.content_type.to_string(), data.content)
        .with_metadata(desc);
    trace!("create context for question: {}", context);
    state
      .ai_client
      .create_chat_text_context(context)
      .await
      .map_err(AppError::from)?;
  }

  let uid = state.user_cache.get_user_uid(&uuid).await?;
  let resp = create_chat_message(&state.pg_pool, uid, chat_id, params).await?;
  Ok(AppResponse::Ok().with_data(resp).into())
}

#[instrument(level = "debug", skip_all, err)]
async fn create_question_handler_v2(
  _state: Data<AppState>,
  _path: web::Path<(String, String)>,
  _payload: Json<CreateChatMessageParamsV2>,
  _uuid: UserUuid,
) -> actix_web::Result<JsonAppResponse<ChatMessage>> {
  todo!()
}

async fn save_answer_handler(
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
    payload.metadata,
    payload.question_message_id,
  )
  .await?;

  Ok(AppResponse::Ok().with_data(message).into())
}
async fn answer_handler(
  path: web::Path<(String, String, i64)>,
  state: Data<AppState>,
  req: HttpRequest,
) -> actix_web::Result<JsonAppResponse<ChatMessage>> {
  let (_workspace_id, chat_id, message_id) = path.into_inner();
  let ai_model = ai_model_from_header(&req);
  let message = generate_chat_message_answer(
    &state.pg_pool,
    state.ai_client.clone(),
    message_id,
    &chat_id,
    ai_model,
  )
  .await?;
  Ok(AppResponse::Ok().with_data(message).into())
}

#[instrument(level = "debug", skip_all, err)]
async fn answer_stream_handler(
  path: web::Path<(String, String, i64)>,
  state: Data<AppState>,
  req: HttpRequest,
) -> actix_web::Result<HttpResponse> {
  let (_workspace_id, chat_id, question_id) = path.into_inner();
  let (content, metadata) =
    chat::chat_ops::select_chat_message_content(&state.pg_pool, question_id).await?;
  let rag_ids = chat::chat_ops::select_chat_rag_ids(&state.pg_pool, &chat_id).await?;
  let ai_model = ai_model_from_header(&req);
  match state
    .ai_client
    .stream_question(&chat_id, &content, Some(metadata), rag_ids, &ai_model)
    .await
  {
    Ok(answer_stream) => {
      let new_answer_stream = answer_stream.map_err(AppError::from);
      Ok(
        HttpResponse::Ok()
          .content_type("text/event-stream")
          .streaming(new_answer_stream),
      )
    },
    Err(err) => Ok(
      HttpResponse::Ok()
        .content_type("text/event-stream")
        .streaming(stream::once(async move {
          Err(AppError::AIServiceUnavailable(err.to_string()))
        })),
    ),
  }
}

#[instrument(level = "debug", skip_all, err)]
async fn answer_stream_v2_handler(
  path: web::Path<(String, String, i64)>,
  state: Data<AppState>,
  req: HttpRequest,
) -> actix_web::Result<HttpResponse> {
  let (_workspace_id, chat_id, question_id) = path.into_inner();
  let (content, metadata) =
    chat::chat_ops::select_chat_message_content(&state.pg_pool, question_id).await?;
  let rag_ids = chat::chat_ops::select_chat_rag_ids(&state.pg_pool, &chat_id).await?;
  let ai_model = ai_model_from_header(&req);

  trace!(
    "[Chat] stream answer for chat: {}, question: {}, rag_ids: {:?}",
    chat_id,
    content,
    rag_ids
  );
  match state
    .ai_client
    .stream_question_v2(&chat_id, &content, Some(metadata), rag_ids, &ai_model)
    .await
  {
    Ok(answer_stream) => {
      let new_answer_stream = answer_stream.map_err(AppError::from);
      Ok(
        HttpResponse::Ok()
          .content_type("text/event-stream")
          .streaming(new_answer_stream),
      )
    },
    Err(err) => Ok(
      HttpResponse::Ok()
        .content_type("text/event-stream")
        .streaming(stream::once(async move {
          Err(AppError::AIServiceUnavailable(err.to_string()))
        })),
    ),
  }
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

#[allow(dead_code)]
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
            this.buffer.extend_from_slice(&bytes);
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
            Poll::Ready(Ok(_)) => {
              this.result_receiver.take();
              Poll::Ready(None)
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
