use actix_http::Payload;
use actix_web::{FromRequest, HttpRequest};
use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
use serde::{Deserialize, Serialize};

lazy_static::lazy_static! {
  pub static ref VALIDATION: Validation = Validation::new(Algorithm::HS256);
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AuthorizationBearer {
  pub token: String,
  pub claims: GoTrueJWTClaims,
}

impl FromRequest for AuthorizationBearer {
  type Error = actix_web::Error;

  type Future = std::future::Ready<Result<Self, Self::Error>>;

  fn from_request(req: &HttpRequest, _payload: &mut Payload) -> Self::Future {
    let bearer = req.headers().get("Authorization");
    match bearer {
      Some(bearer) => {
        let bearer = bearer.to_str().unwrap();
        let token = bearer.split_once("Bearer ").unwrap().1;
        let claim = decode_from_str(token, "todo_secret".as_bytes());
        let auth_bearer = Self {
          token: token.to_string(),
          claims: claim.unwrap(),
        };
        std::future::ready(Ok(auth_bearer))
      },
      None => std::future::ready(Err(actix_web::error::ErrorUnauthorized(
        "No Authorization header",
      ))),
    }
  }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GoTrueJWTClaims {
  // JWT standard claims
  aud: Option<String>,
  exp: Option<i64>,
  jti: Option<String>,
  iat: Option<i64>,
  iss: Option<String>,
  nbf: Option<i64>,
  sub: Option<String>,

  email: String,
  phone: String,
  app_metadata: serde_json::Value,
  user_metadata: serde_json::Value,
  role: String,
  aal: Option<String>,
  amr: Option<Vec<Amr>>,
  session_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Amr {
  method: String,
  timestamp: u64,
  provider: Option<String>,
}

fn decode_from_str(
  token: &str,
  secret: &[u8],
) -> Result<GoTrueJWTClaims, jsonwebtoken::errors::Error> {
  Ok(decode(token, &DecodingKey::from_secret(secret), &VALIDATION)?.claims)
}
