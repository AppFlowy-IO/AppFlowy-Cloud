use chrono::{DateTime, Duration, Utc};
use hmac::digest::KeyInit;
use hmac::Hmac;
use jwt::{SignWithKey, VerifyWithKey};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::Sha256;

#[derive(Debug, thiserror::Error)]
pub enum TokenError {
  #[error(transparent)]
  Jwt(#[from] jwt::Error),

  #[error("Token expired")]
  Expired,
}

#[derive(Debug, Serialize, Deserialize, Eq, PartialEq)]
pub enum TokenType {
  AccessToken,
}

#[derive(Debug, Serialize, Deserialize)]
struct TokenFields<T> {
  #[serde(rename = "d")]
  data: T,

  #[serde(rename = "exp")]
  expire_at: DateTime<Utc>,
}

pub fn create_token(
  server_key: &str,
  data: impl Serialize,
  expire_duration: Duration,
) -> Result<String, TokenError> {
  Ok(
    TokenFields {
      data,
      expire_at: Utc::now() + expire_duration,
    }
    .sign_with_key(&generate_hmac_key(server_key))?,
  )
}

fn generate_hmac_key(server_key: &str) -> Hmac<Sha256> {
  Hmac::<Sha256>::new_from_slice(server_key.as_bytes()).expect("invalid server key")
}

pub fn parse_token<T: DeserializeOwned>(server_key: &str, token: &str) -> Result<T, TokenError> {
  let fields =
    VerifyWithKey::<TokenFields<T>>::verify_with_key(token, &generate_hmac_key(server_key))?;
  if fields.expire_at < Utc::now() {
    return Err(TokenError::Expired);
  }
  Ok(fields.data)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn create_token_test() {
    let token_data = "hello appflowy".to_string();
    let token = create_token("server_key", &token_data, Duration::days(2)).unwrap();

    let parse_token_data = parse_token::<String>("server_key", &token).unwrap();
    assert_eq!(token_data, parse_token_data);
  }

  #[test]
  #[should_panic]
  fn parser_token_with_different_server_key() {
    let server_key = "123456";
    let token = create_token(server_key, "hello", Duration::days(2)).unwrap();
    let _ = parse_token::<String>("abcdef", &token).unwrap();
  }
}
