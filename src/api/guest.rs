use actix_web::{
  web::{Data, Json},
  Result,
};
use app_error::ErrorCode;
use shared_entity::{
  dto::guest_dto::{
    RevokeSharedViewAccessRequest, ShareViewWithGuestRequest, SharedViewDetails,
    SharedViewDetailsRequest, SharedViews,
  },
  response::{AppResponseError, JsonAppResponse},
};

use actix_web::{
  web::{self},
  Scope,
};
use uuid::Uuid;

use crate::biz::authentication::jwt::UserUuid;
use crate::state::AppState;

pub fn sharing_scope() -> Scope {
  web::scope("/api/sharing/workspace")
    .service(
      web::resource("{workspace_id}/view")
        .route(web::get().to(list_shared_views_handler))
        .route(web::put().to(put_shared_view_handler)),
    )
    .service(
      web::resource("{workspace_id}/view/{view_id}/access-details")
        .route(web::post().to(shared_view_access_details_handler)),
    )
    .service(
      web::resource("{workspace_id}/view/{view_id}/revoke-access")
        .route(web::post().to(revoke_shared_view_access_handler)),
    )
}

async fn list_shared_views_handler(
  _user_uuid: UserUuid,
  _state: Data<AppState>,
  _path: web::Path<Uuid>,
) -> Result<JsonAppResponse<SharedViews>> {
  Err(
    AppResponseError::new(
      ErrorCode::FeatureNotAvailable,
      "this version of appflowy cloud server does not support guest editors",
    )
    .into(),
  )
}

async fn put_shared_view_handler(
  _user_uuid: UserUuid,
  _state: Data<AppState>,
  _payload: web::Json<ShareViewWithGuestRequest>,
  _path: web::Path<Uuid>,
) -> Result<JsonAppResponse<()>> {
  Err(
    AppResponseError::new(
      ErrorCode::FeatureNotAvailable,
      "this version of appflowy cloud server does not support guest editors",
    )
    .into(),
  )
}

async fn shared_view_access_details_handler(
  _user_uuid: UserUuid,
  _state: Data<AppState>,
  _json: Json<SharedViewDetailsRequest>,
  _path: web::Path<(Uuid, Uuid)>,
) -> Result<JsonAppResponse<SharedViewDetails>> {
  Err(
    AppResponseError::new(
      ErrorCode::FeatureNotAvailable,
      "this version of appflowy cloud server does not support guest editors",
    )
    .into(),
  )
}

async fn revoke_shared_view_access_handler(
  _user_uuid: UserUuid,
  _state: Data<AppState>,
  _payload: web::Json<RevokeSharedViewAccessRequest>,
  _path: web::Path<(Uuid, Uuid)>,
) -> Result<JsonAppResponse<()>> {
  Err(
    AppResponseError::new(
      ErrorCode::FeatureNotAvailable,
      "this version of appflowy cloud server does not support guest editors",
    )
    .into(),
  )
}
