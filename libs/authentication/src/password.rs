use actix_web::rt::task::JoinHandle;
use anyhow::Context;
use argon2::password_hash::SaltString;
use argon2::{Algorithm, Argon2, Params, PasswordHash, PasswordHasher, PasswordVerifier, Version};
use secrecy::{ExposeSecret, Secret};

use crate::error::AuthError;
use sqlx::PgPool;

pub struct Credentials {
  pub email: String,
  pub password: Secret<String>,
}

#[tracing::instrument(level = "debug", skip(credentials, pool))]
pub async fn validate_credentials(
  credentials: Credentials,
  pool: &PgPool,
) -> Result<i64, AuthError> {
  let mut uid = None;
  let mut expected_hash_password = Secret::new(
    "$argon2id$v=19$m=15000,t=2,p=1$\
        gZiV/M1gPc22ElAH/Jh1Hw$\
        CWOrkoo7oJBQ/iyh7uJ0LO2aLEfrHwTWllSAxT0zRno"
      .to_string(),
  );

  if let Some((stored_uid, stored_hash_password)) =
    get_stored_credentials(&credentials.email, pool).await?
  {
    uid = Some(stored_uid);
    expected_hash_password = stored_hash_password;
  }

  spawn_blocking_with_tracing(move || {
    verify_password_hash(expected_hash_password, credentials.password)
  })
  .await
  .context("Failed to spawn blocking task.")??;

  uid
    .ok_or_else(|| anyhow::anyhow!("Unknown email."))
    .map_err(AuthError::InvalidCredentials)
}

pub fn compute_hash_password(password: &[u8]) -> Result<Secret<String>, anyhow::Error> {
  let salt = SaltString::generate(&mut rand::thread_rng());
  let password = Argon2::new(
    Algorithm::Argon2id,
    Version::V0x13,
    Params::new(15000, 2, 1, None).unwrap(),
  )
  .hash_password(password, &salt)?
  .to_string();
  Ok(Secret::new(password))
}

#[tracing::instrument(level = "debug", skip(email, pool))]
async fn get_stored_credentials(
  email: &str,
  pool: &PgPool,
) -> Result<Option<(i64, Secret<String>)>, anyhow::Error> {
  let row = sqlx::query!(
    r#"
        SELECT uid, password
        FROM af_user
        WHERE email = $1
        "#,
    email,
  )
  .fetch_optional(pool)
  .await
  .context("Failed to performed a query to retrieve stored credentials.")?
  .map(|row| (row.uid, Secret::new(row.password)));
  Ok(row)
}

fn verify_password_hash(
  expected_password_hash: Secret<String>,
  password_candidate: Secret<String>,
) -> Result<(), AuthError> {
  let expected_hash_password = PasswordHash::new(expected_password_hash.expose_secret())
    .context("Failed to parse hash in PHC string format.")?;

  Argon2::default()
    .verify_password(
      password_candidate.expose_secret().as_bytes(),
      &expected_hash_password,
    )
    .context("Invalid password.")
    .map_err(|_| AuthError::InvalidPassword)
}

pub fn spawn_blocking_with_tracing<F, R>(f: F) -> JoinHandle<R>
where
  F: FnOnce() -> R + Send + 'static,
  R: Send + 'static,
{
  let current_span = tracing::Span::current();
  actix_web::rt::task::spawn_blocking(move || current_span.in_scope(f))
}
