use anyhow::Context;
use std::ops::DerefMut;

use secrecy::zeroize::DefaultIsZeroes;
use secrecy::{CloneableSecret, DebugSecret};
use serde::{Deserialize, Serialize};
use sqlx::{Postgres, Transaction};

pub async fn get_user_email(
  uid: i64,
  transaction: &mut Transaction<'_, Postgres>,
) -> Result<String, anyhow::Error> {
  let row = sqlx::query!(
    r#"
        SELECT email
        FROM af_user
        WHERE uid = $1
        "#,
    uid,
  )
  .fetch_one(transaction.deref_mut())
  .await
  .context("Failed to retrieve the user email`")?;
  Ok(row.email)
}

#[derive(Default, Deserialize, Debug)]
pub struct LoginRequest {
  pub email: String,
  pub password: String,
}

#[derive(Default, Serialize, Deserialize, Debug)]
pub struct LoginResponse {
  pub token: String,
  pub uid: String,
}

#[derive(Default, Deserialize, Debug)]
pub struct RegisterRequest {
  pub email: String,
  pub password: String,
  pub name: String,
}

#[derive(Default, Serialize, Deserialize, Debug)]
pub struct RegisterResponse {
  pub token: String,
}

#[derive(Default, Deserialize, Debug)]
pub struct ChangePasswordRequest {
  pub current_password: String,
  pub new_password: String,
  pub new_password_confirm: String,
}

#[derive(Clone, Default)]
pub struct SecretI64(i64);
impl Copy for SecretI64 {}
impl DefaultIsZeroes for SecretI64 {}
impl DebugSecret for SecretI64 {}
impl CloneableSecret for SecretI64 {}

impl std::ops::Deref for SecretI64 {
  type Target = i64;

  fn deref(&self) -> &Self::Target {
    &self.0
  }
}
