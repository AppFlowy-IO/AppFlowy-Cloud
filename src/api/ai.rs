use crate::api::util::ai_model_from_header;
use crate::state::AppState;
use actix_web::web::{Data, Json};
use actix_web::{web, HttpRequest, HttpResponse, Scope};
use app_error::AppError;
use appflowy_ai_client::dto::{CompleteTextResponse, TranslateRowParams, TranslateRowResponse};
use futures_util::TryStreamExt;
use shared_entity::dto::ai_dto::{
  CompleteTextParams, SummarizeRowData, SummarizeRowParams, SummarizeRowResponse,
};
use shared_entity::response::{AppResponse, JsonAppResponse};

use tracing::{error, instrument};

pub fn ai_completion_scope() -> Scope {
  web::scope("/api/ai/{workspace_id}")
    .service(web::resource("/complete").route(web::post().to(complete_text_handler)))
    .service(web::resource("/complete/stream").route(web::post().to(stream_complete_text_handler)))
    .service(web::resource("/summarize_row").route(web::post().to(summarize_row_handler)))
    .service(web::resource("/translate_row").route(web::post().to(translate_row_handler)))
}

async fn complete_text_handler(
  state: Data<AppState>,
  payload: Json<CompleteTextParams>,
  req: HttpRequest,
) -> actix_web::Result<JsonAppResponse<CompleteTextResponse>> {
  let ai_model = ai_model_from_header(&req);
  let params = payload.into_inner();
  let resp = state
    .ai_client
    .completion_text(&params.text, params.completion_type, ai_model)
    .await
    .map_err(|err| AppError::Internal(err.into()))?;
  Ok(AppResponse::Ok().with_data(resp).into())
}

async fn stream_complete_text_handler(
  state: Data<AppState>,
  payload: Json<CompleteTextParams>,
  req: HttpRequest,
) -> actix_web::Result<HttpResponse> {
  let ai_model = ai_model_from_header(&req);
  let params = payload.into_inner();
  let stream = state
    .ai_client
    .stream_completion_text(&params.text, params.completion_type, ai_model)
    .await
    .map_err(|err| AppError::Internal(err.into()))?;
  Ok(
    HttpResponse::Ok()
      .content_type("text/event-stream")
      .streaming(stream.map_err(AppError::from)),
  )
}

#[instrument(level = "debug", skip(state, payload), err)]
async fn summarize_row_handler(
  state: Data<AppState>,
  payload: Json<SummarizeRowParams>,
  req: HttpRequest,
) -> actix_web::Result<Json<AppResponse<SummarizeRowResponse>>> {
  let params = payload.into_inner();
  match params.data {
    SummarizeRowData::Identity { .. } => {
      return Err(AppError::InvalidRequest("Identity data is not supported".to_string()).into());
    },
    SummarizeRowData::Content(content) => {
      if content.is_empty() {
        return Ok(
          AppResponse::Ok()
            .with_data(SummarizeRowResponse {
              text: "No content".to_string(),
            })
            .into(),
        );
      }

      let ai_model = ai_model_from_header(&req);
      let result = state.ai_client.summarize_row(&content, ai_model).await;
      let resp = match result {
        Ok(resp) => SummarizeRowResponse { text: resp.text },
        Err(err) => {
          error!("Failed to summarize row: {:?}", err);
          SummarizeRowResponse {
            text: "No content".to_string(),
          }
        },
      };

      Ok(AppResponse::Ok().with_data(resp).into())
    },
  }
}

#[instrument(level = "debug", skip(state, payload), err)]
async fn translate_row_handler(
  state: web::Data<AppState>,
  payload: web::Json<TranslateRowParams>,
  req: HttpRequest,
) -> actix_web::Result<Json<AppResponse<TranslateRowResponse>>> {
  let params = payload.into_inner();
  let ai_model = ai_model_from_header(&req);
  match state.ai_client.translate_row(params.data, ai_model).await {
    Ok(resp) => Ok(AppResponse::Ok().with_data(resp).into()),
    Err(err) => {
      error!("Failed to translate row: {:?}", err);
      Ok(
        AppResponse::Ok()
          .with_data(TranslateRowResponse::default())
          .into(),
      )
    },
  }
}
