use actix_multipart::form::{bytes::Bytes as MPBytes, MultipartForm};
use actix_web::http::StatusCode;
use actix_web::{
  web::{self, Data, Json},
  HttpResponse, Result, Scope,
};

use database_entity::dto::{
  AvatarImageSource, CreateTemplateCategoryParams, CreateTemplateCreatorParams,
  CreateTemplateParams, GetTemplateCategoriesQueryParams, GetTemplateCreatorsQueryParams,
  GetTemplatesQueryParams, Template, TemplateCategories, TemplateCategory, TemplateCreator,
  TemplateCreators, TemplateHomePage, TemplateHomePageQueryParams, TemplateWithPublishInfo,
  Templates, UpdateTemplateCategoryParams, UpdateTemplateCreatorParams, UpdateTemplateParams,
};
use shared_entity::response::{AppResponse, JsonAppResponse};
use uuid::Uuid;

use crate::biz::authentication::jwt::UserUuid;
use crate::{biz::template::ops::*, state::AppState};

pub fn template_scope() -> Scope {
  web::scope("/api/template-center")
    .service(
      web::resource("/category")
        .route(web::post().to(post_template_category_handler))
        .route(web::get().to(list_template_categories_handler)),
    )
    .service(
      web::resource("/category/{category_id}")
        .route(web::put().to(update_template_category_handler))
        .route(web::get().to(get_template_category_handler))
        .route(web::delete().to(delete_template_category_handler)),
    )
    .service(
      web::resource("/creator")
        .route(web::post().to(post_template_creator_handler))
        .route(web::get().to(list_template_creators_handler)),
    )
    .service(
      web::resource("/creator/{creator_id}")
        .route(web::put().to(update_template_creator_handler))
        .route(web::get().to(get_template_creator_handler))
        .route(web::delete().to(delete_template_creator_handler)),
    )
    .service(
      web::resource("/template")
        .route(web::post().to(post_template_handler))
        .route(web::get().to(list_templates_handler)),
    )
    .service(
      web::resource("/template/{view_id}")
        .route(web::put().to(update_template_handler))
        .route(web::get().to(get_template_handler))
        .route(web::delete().to(delete_template_handler)),
    )
    .service(web::resource("/homepage").route(web::get().to(get_template_homepage_handler)))
    .service(web::resource("/avatar").route(web::put().to(put_avatar_handler)))
    .service(web::resource("/avatar/{avatar_id}").route(web::get().to(get_avatar_handler)))
}

async fn post_template_category_handler(
  _uuid: UserUuid,
  data: Json<CreateTemplateCategoryParams>,
  state: Data<AppState>,
) -> Result<JsonAppResponse<TemplateCategory>> {
  let new_template_category = create_new_template_category(
    &state.pg_pool,
    &data.name,
    &data.description,
    &data.icon,
    &data.bg_color,
    data.category_type,
    data.priority,
  )
  .await?;
  Ok(Json(AppResponse::Ok().with_data(new_template_category)))
}

async fn list_template_categories_handler(
  query: web::Query<GetTemplateCategoriesQueryParams>,
  state: Data<AppState>,
) -> Result<JsonAppResponse<TemplateCategories>> {
  let categories = get_template_categories(
    &state.pg_pool,
    query.name_contains.as_deref(),
    query.category_type,
  )
  .await?;
  Ok(Json(
    AppResponse::Ok().with_data(TemplateCategories { categories }),
  ))
}

async fn update_template_category_handler(
  _uuid: UserUuid,
  category_id: web::Path<Uuid>,
  data: Json<UpdateTemplateCategoryParams>,
  state: Data<AppState>,
) -> Result<JsonAppResponse<TemplateCategory>> {
  let category_id = category_id.into_inner();
  let updated_template_category = update_template_category(
    &state.pg_pool,
    category_id,
    &data.name,
    &data.description,
    &data.icon,
    &data.bg_color,
    data.category_type,
    data.priority,
  )
  .await?;
  Ok(Json(AppResponse::Ok().with_data(updated_template_category)))
}

async fn get_template_category_handler(
  category_id: web::Path<Uuid>,
  state: Data<AppState>,
) -> Result<JsonAppResponse<TemplateCategory>> {
  let category_id = category_id.into_inner();
  let category = get_template_category(&state.pg_pool, category_id).await?;
  Ok(Json(AppResponse::Ok().with_data(category)))
}

async fn delete_template_category_handler(
  _uuid: UserUuid,
  category_id: web::Path<Uuid>,
  state: Data<AppState>,
) -> Result<JsonAppResponse<()>> {
  let category_id = category_id.into_inner();
  delete_template_category(&state.pg_pool, category_id).await?;
  Ok(Json(AppResponse::Ok()))
}

async fn post_template_creator_handler(
  _uuid: UserUuid,
  data: Json<CreateTemplateCreatorParams>,
  state: Data<AppState>,
) -> Result<JsonAppResponse<TemplateCreator>> {
  let new_template_creator = create_new_template_creator(
    &state.pg_pool,
    &data.name,
    &data.avatar_url,
    &data.account_links,
  )
  .await?;
  Ok(Json(AppResponse::Ok().with_data(new_template_creator)))
}

async fn list_template_creators_handler(
  query: web::Query<GetTemplateCreatorsQueryParams>,
  state: Data<AppState>,
) -> Result<JsonAppResponse<TemplateCreators>> {
  let creators = get_template_creators(&state.pg_pool, &query.name_contains).await?;
  Ok(Json(
    AppResponse::Ok().with_data(TemplateCreators { creators }),
  ))
}

async fn update_template_creator_handler(
  _uuid: UserUuid,
  creator_id: web::Path<Uuid>,
  data: Json<UpdateTemplateCreatorParams>,
  state: Data<AppState>,
) -> Result<JsonAppResponse<TemplateCreator>> {
  let creator_id = creator_id.into_inner();
  let updated_creator = update_template_creator(
    &state.pg_pool,
    creator_id,
    &data.name,
    &data.avatar_url,
    &data.account_links,
  )
  .await?;
  Ok(Json(AppResponse::Ok().with_data(updated_creator)))
}

async fn get_template_creator_handler(
  creator_id: web::Path<Uuid>,
  state: Data<AppState>,
) -> Result<JsonAppResponse<TemplateCreator>> {
  let creator_id = creator_id.into_inner();
  let template_creator = get_template_creator(&state.pg_pool, creator_id).await?;
  Ok(Json(AppResponse::Ok().with_data(template_creator)))
}

async fn delete_template_creator_handler(
  _uuid: UserUuid,
  creator_id: web::Path<Uuid>,
  state: Data<AppState>,
) -> Result<JsonAppResponse<TemplateCreator>> {
  let creator_id = creator_id.into_inner();
  delete_template_creator(&state.pg_pool, creator_id).await?;
  Ok(Json(AppResponse::Ok()))
}

async fn post_template_handler(
  _uuid: UserUuid,
  data: Json<CreateTemplateParams>,
  state: Data<AppState>,
) -> Result<JsonAppResponse<Template>> {
  let new_template = create_new_template(
    &state.pg_pool,
    data.view_id,
    &data.name,
    &data.description,
    &data.about,
    &data.view_url,
    data.creator_id,
    data.is_new_template,
    data.is_featured,
    &data.category_ids,
    &data.related_view_ids,
  )
  .await?;
  Ok(Json(AppResponse::Ok().with_data(new_template)))
}

async fn list_templates_handler(
  data: web::Query<GetTemplatesQueryParams>,
  state: Data<AppState>,
) -> Result<JsonAppResponse<Templates>> {
  let data = data.into_inner();
  let template_summary_list = get_templates_with_publish_info(
    &state.pg_pool,
    data.category_id,
    data.is_featured,
    data.is_new_template,
    data.name_contains.as_deref(),
  )
  .await?;
  Ok(Json(AppResponse::Ok().with_data(Templates {
    templates: template_summary_list,
  })))
}

async fn get_template_handler(
  view_id: web::Path<Uuid>,
  state: Data<AppState>,
) -> Result<JsonAppResponse<TemplateWithPublishInfo>> {
  let view_id = view_id.into_inner();
  let template_with_pub_info = get_template_with_publish_info(&state.pg_pool, view_id).await?;
  Ok(Json(AppResponse::Ok().with_data(template_with_pub_info)))
}

async fn update_template_handler(
  _uuid: UserUuid,
  view_id: web::Path<Uuid>,
  data: Json<UpdateTemplateParams>,
  state: Data<AppState>,
) -> Result<JsonAppResponse<Template>> {
  let view_id = view_id.into_inner();
  let updated_template = update_template(
    &state.pg_pool,
    view_id,
    &data.name,
    &data.description,
    &data.about,
    &data.view_url,
    data.creator_id,
    data.is_new_template,
    data.is_featured,
    &data.category_ids,
    &data.related_view_ids,
  )
  .await?;
  Ok(Json(AppResponse::Ok().with_data(updated_template)))
}

async fn delete_template_handler(
  _uuid: UserUuid,
  view_id: web::Path<Uuid>,
  state: Data<AppState>,
) -> Result<JsonAppResponse<()>> {
  let view_id = view_id.into_inner();
  delete_template(&state.pg_pool, view_id).await?;
  Ok(Json(AppResponse::Ok()))
}

async fn get_template_homepage_handler(
  query: web::Query<TemplateHomePageQueryParams>,
  state: Data<AppState>,
) -> Result<JsonAppResponse<TemplateHomePage>> {
  let template_homepage = get_template_homepage(&state.pg_pool, query.per_count).await?;
  Ok(Json(AppResponse::Ok().with_data(template_homepage)))
}

#[derive(MultipartForm)]
#[multipart(duplicate_field = "deny")]
struct UploadAvatarForm {
  #[multipart(limit = "150KB")]
  avatar: MPBytes,
}

async fn get_avatar_handler(
  file_id: web::Path<String>,
  state: Data<AppState>,
) -> Result<HttpResponse> {
  let file_id = file_id.into_inner();
  let avatar = get_avatar(state.bucket_client.clone(), file_id).await?;
  Ok(
    HttpResponse::build(StatusCode::OK)
      .content_type(avatar.content_type)
      .body(avatar.data),
  )
}

async fn put_avatar_handler(
  _uuid: UserUuid,
  MultipartForm(form): MultipartForm<UploadAvatarForm>,
  state: Data<AppState>,
) -> Result<JsonAppResponse<AvatarImageSource>> {
  let file_id = upload_avatar(state.bucket_client.clone(), &form.avatar).await?;
  Ok(Json(
    AppResponse::Ok().with_data(AvatarImageSource { file_id }),
  ))
}
