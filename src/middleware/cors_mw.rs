use actix_cors::Cors;
use actix_web::http;

// Deprecated
// AppFlowy Cloud uses nginx to configure CORS
pub fn default_cors() -> Cors {
  Cors::default() // allowed_origin return access-control-allow-origin: * by default
    .allow_any_origin()
    .send_wildcard()
    .allowed_methods(vec!["GET", "POST", "PUT", "DELETE"])
    .allowed_headers(vec![
      actix_web::http::header::AUTHORIZATION,
      actix_web::http::header::ACCEPT,
    ])
    .allowed_header(http::header::CONTENT_TYPE)
    .max_age(3600)
}
