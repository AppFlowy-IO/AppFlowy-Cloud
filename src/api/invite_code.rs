use actix_web::{
  web::{self, Data, Json},
  Result, Scope,
};
use authentication::jwt::UserUuid;
use database_entity::dto::{GetInvitationCodeInfoQuery, InvitationCodeInfo};
use shared_entity::response::{AppResponse, JsonAppResponse};

use crate::{biz::workspace::invite::get_invitation_code_info, state::AppState};

pub fn invite_code_scope() -> Scope {
  web::scope("/api/invite-code-info")
    .service(web::resource("").route(web::get().to(get_invite_code_info_handler)))
}

async fn get_invite_code_info_handler(
  user_uuid: UserUuid,
  query: web::Query<GetInvitationCodeInfoQuery>,
  state: Data<AppState>,
) -> Result<JsonAppResponse<InvitationCodeInfo>> {
  let uid = state.user_cache.get_user_uid(&user_uuid).await?;
  let info = get_invitation_code_info(&state.pg_pool, &query.code, uid).await?;
  Ok(Json(AppResponse::Ok().with_data(info)))
}
