use anyhow::Error;
use std::env;

use crate::{component::auth::gotrue::models::SignUpResponse, utils::http_response::from_response};

lazy_static::lazy_static!(
    static ref GOTRUE_BASE_URL: String = env::var("GOTRUE_BASE_URL")
        .expect("GOTRUE_BASE_URL must be set");
    static ref SIGNUP_URL: String = format!("{}/signup", GOTRUE_BASE_URL.as_str());
);

pub async fn sign_up(
  client: reqwest::Client,
  email: &str,
  password: &str,
) -> Result<SignUpResponse, Error> {
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
