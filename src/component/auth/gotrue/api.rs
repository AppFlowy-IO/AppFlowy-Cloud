use super::{
  grant::Grant,
  models::{AccessTokenResponse, User},
};
use crate::utils::http_response::from_response;
use anyhow::Error;
use std::env;

lazy_static::lazy_static!(
    static ref GOTRUE_BASE_URL: String = env::var("GOTRUE_BASE_URL")
        .expect("GOTRUE_BASE_URL must be set");
    static ref SIGNUP_URL: String = format!("{}/signup", GOTRUE_BASE_URL.as_str());
    static ref TOKEN_URL: String = format!("{}/token", GOTRUE_BASE_URL.as_str());
);

pub async fn sign_up(client: reqwest::Client, email: &str, password: &str) -> Result<User, Error> {
  let payload = serde_json::json!({
      "email": email,
      "password": password,
  });
  let resp = client
    .post(SIGNUP_URL.as_str())
    .json(&payload)
    .send()
    .await?;
  from_response(resp).await
}

pub async fn token(client: reqwest::Client, grant: &Grant) -> Result<AccessTokenResponse, Error> {
  let url = format!("{}?grant_type={}", TOKEN_URL.as_str(), grant.type_as_str());
  let payload = grant.json_value();
  let resp = client.post(url.as_str()).json(&payload).send().await?;
  from_response(resp).await
}
