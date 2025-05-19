use crate::api::util::ai_model_from_header;
use crate::state::AppState;

use actix_web::web::{Data, Json};
use actix_web::{web, HttpRequest, HttpResponse, Scope};
use app_error::AppError;
use appflowy_ai_client::dto::{
  CalculateSimilarityParams, LocalAIConfig, ModelList, SimilarityResponse, TranslateRowParams,
  TranslateRowResponse,
};

use futures_util::{stream, TryStreamExt};

use serde::Deserialize;
use shared_entity::dto::ai_dto::{
  CompleteTextParams, SummarizeRowData, SummarizeRowParams, SummarizeRowResponse,
};
use shared_entity::response::AppResponse;

use tracing::{error, instrument, trace};

pub fn ai_completion_scope() -> Scope {
  web::scope("/api/ai/{workspace_id}")
    .service(web::resource("/complete/stream").route(web::post().to(stream_complete_text_handler)))
    .service(web::resource("/v2/complete/stream").route(web::post().to(stream_complete_v2_handler)))
    .service(web::resource("/summarize_row").route(web::post().to(summarize_row_handler)))
    .service(web::resource("/translate_row").route(web::post().to(translate_row_handler)))
    .service(web::resource("/local/config").route(web::get().to(local_ai_config_handler)))
    .service(
      web::resource("/calculate_similarity").route(web::post().to(calculate_similarity_handler)),
    )
    .service(web::resource("/model/list").route(web::get().to(model_list_handler)))
}

async fn stream_complete_text_handler(
  state: Data<AppState>,
  payload: Json<CompleteTextParams>,
  req: HttpRequest,
) -> actix_web::Result<HttpResponse> {
  let ai_model = ai_model_from_header(&req);
  let params = payload.into_inner();
  state.metrics.ai_metrics.record_total_completion_count(1);

  if let Some(prompt_id) = params
    .metadata
    .as_ref()
    .and_then(|metadata| metadata.prompt_id.as_ref())
  {
    state
      .metrics
      .ai_metrics
      .record_prompt_usage_count(prompt_id, 1);
  }

  match state
    .ai_client
    .stream_completion_text(params, ai_model)
    .await
  {
    Ok(stream) => Ok(
      HttpResponse::Ok()
        .content_type("text/event-stream")
        .streaming(stream.map_err(AppError::from)),
    ),
    Err(err) => Ok(
      HttpResponse::Ok()
        .content_type("text/event-stream")
        .streaming(stream::once(async move {
          Err(AppError::AIServiceUnavailable(err.to_string()))
        })),
    ),
  }
}

async fn stream_complete_v2_handler(
  state: Data<AppState>,
  payload: Json<CompleteTextParams>,
  req: HttpRequest,
) -> actix_web::Result<HttpResponse> {
  let ai_model = ai_model_from_header(&req);
  let params = payload.into_inner();
  state.metrics.ai_metrics.record_total_completion_count(1);

  match state.ai_client.stream_completion_v2(params, ai_model).await {
    Ok(stream) => Ok(
      HttpResponse::Ok()
        .content_type("text/event-stream")
        .streaming(stream.map_err(AppError::from)),
    ),
    Err(err) => Ok(
      HttpResponse::Ok()
        .content_type("text/event-stream")
        .streaming(stream::once(async move {
          Err(AppError::AIServiceUnavailable(err.to_string()))
        })),
    ),
  }
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

      state.metrics.ai_metrics.record_total_summary_row_count(1);
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
  state.metrics.ai_metrics.record_total_translate_row_count(1);
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

#[derive(Deserialize, Debug)]
struct ConfigQuery {
  platform: String,
  app_version: Option<String>,
}

#[instrument(level = "debug", skip_all, err)]
async fn local_ai_config_handler(
  state: web::Data<AppState>,
  query: web::Query<ConfigQuery>,
) -> actix_web::Result<Json<AppResponse<LocalAIConfig>>> {
  let query = query.into_inner();
  trace!("query ai configuration: {:?}", query);
  let platform = match query.platform.as_str() {
    "macos" => "macos",
    "linux" => "ubuntu",
    "ubuntu" => "ubuntu",
    "windows" => "windows",
    _ => {
      return Err(AppError::InvalidRequest("Invalid platform".to_string()).into());
    },
  };

  let config = state
    .ai_client
    .get_local_ai_config(platform, query.app_version)
    .await
    .map_err(|err| AppError::AIServiceUnavailable(err.to_string()))?;
  Ok(AppResponse::Ok().with_data(config).into())
}

#[instrument(level = "debug", skip_all, err)]
async fn calculate_similarity_handler(
  state: web::Data<AppState>,
  payload: web::Json<CalculateSimilarityParams>,
) -> actix_web::Result<Json<AppResponse<SimilarityResponse>>> {
  let params = payload.into_inner();

  let response = state
    .ai_client
    .calculate_similarity(params)
    .await
    .map_err(|err| AppError::AIServiceUnavailable(err.to_string()))?;
  Ok(AppResponse::Ok().with_data(response).into())
}

#[instrument(level = "debug", skip_all, err)]
async fn model_list_handler(
  state: web::Data<AppState>,
) -> actix_web::Result<Json<AppResponse<ModelList>>> {
  let model_list = state
    .ai_client
    .get_model_list()
    .await
    .map_err(|err| AppError::AIServiceUnavailable(err.to_string()))?;
  Ok(AppResponse::Ok().with_data(model_list).into())
}
