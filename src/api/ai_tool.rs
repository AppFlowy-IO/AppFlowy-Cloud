use crate::state::AppState;
use actix_web::web::{Data, Json};
use actix_web::{web, Scope};
use app_error::AppError;
use appflowy_ai_client::dto::CompleteTextResponse;
use shared_entity::dto::ai_dto::{
  CompleteTextParams, SummarizeRowData, SummarizeRowParams, SummarizeRowResponse,
};
use shared_entity::response::{AppResponse, JsonAppResponse};
use tracing::{error, instrument};

pub fn ai_tool_scope() -> Scope {
  web::scope("/api/ai/{workspace_id}")
    .service(web::resource("/complete_text").route(web::post().to(complete_text_handler)))
    .service(web::resource("/summarize_row").route(web::post().to(summarize_row_handler)))
}

async fn complete_text_handler(
  state: Data<AppState>,
  payload: Json<CompleteTextParams>,
) -> actix_web::Result<JsonAppResponse<CompleteTextResponse>> {
  let params = payload.into_inner();
  let resp = state
    .ai_client
    .completion_text(&params.text, params.completion_type)
    .await
    .map_err(|err| AppError::Internal(err.into()))?;
  Ok(AppResponse::Ok().with_data(resp).into())
}

#[instrument(level = "debug", skip(state, payload), err)]
async fn summarize_row_handler(
  state: Data<AppState>,
  payload: Json<SummarizeRowParams>,
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

      let result = state.ai_client.summarize_row(&content).await;
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
