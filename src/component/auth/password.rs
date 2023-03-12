use crate::telemetry::spawn_blocking_with_tracing;
use anyhow::Context;
use argon2::password_hash::SaltString;
use argon2::{Algorithm, Argon2, Params, PasswordHash, PasswordHasher, PasswordVerifier, Version};
use secrecy::{ExposeSecret, Secret};
use sqlx::PgPool;

pub struct Credentials {
    pub username: String,
    pub password: Secret<String>,
}

#[tracing::instrument(name = "Validate credentials", skip(credentials, pool))]
pub async fn validate_credentials(
    credentials: Credentials,
    pool: &PgPool,
) -> Result<uuid::Uuid, anyhow::Error> {
    let mut user_id = None;
    let mut expected_hash_password = Secret::new(
        "$argon2id$v=19$m=15000,t=2,p=1$\
        gZiV/M1gPc22ElAH/Jh1Hw$\
        CWOrkoo7oJBQ/iyh7uJ0LO2aLEfrHwTWllSAxT0zRno"
            .to_string(),
    );

    if let Some((stored_user_id, stored_hash_password)) =
        get_stored_credentials(&credentials.username, pool).await?
    {
        user_id = Some(stored_user_id);
        expected_hash_password = stored_hash_password;
    }

    spawn_blocking_with_tracing(move || {
        verify_password_hash(expected_hash_password, credentials.password)
    })
    .await
    .context("Failed to spawn blocking task.")??;

    user_id.ok_or_else(|| anyhow::anyhow!("Unknown username."))
}

#[tracing::instrument(name = "Change password", skip(password, pool))]
pub async fn change_password(
    uid: uuid::Uuid,
    password: Secret<String>,
    pool: &PgPool,
) -> Result<(), anyhow::Error> {
    let hash_password = spawn_blocking_with_tracing(move || {
        let s = compute_hash_password(password.expose_secret().as_bytes())?;
        Ok::<Secret<String>, anyhow::Error>(Secret::new(s))
    })
    .await?
    .context("Failed to hash password")?;

    sqlx::query!(
        r#"
        UPDATE users
        SET password= $1
        WHERE uid = $2
        "#,
        hash_password.expose_secret(),
        uid
    )
    .execute(pool)
    .await
    .context("Failed to change user's password in the database.")?;
    Ok(())
}

pub fn compute_hash_password(password: &[u8]) -> Result<String, anyhow::Error> {
    let salt = SaltString::generate(&mut rand::thread_rng());
    Ok(Argon2::new(
        Algorithm::Argon2id,
        Version::V0x13,
        Params::new(15000, 2, 1, None).unwrap(),
    )
    .hash_password(password, &salt)?
    .to_string())
}

#[tracing::instrument(name = "Get stored credentials", skip(email, pool))]
async fn get_stored_credentials(
    email: &str,
    pool: &PgPool,
) -> Result<Option<(uuid::Uuid, Secret<String>)>, anyhow::Error> {
    let row = sqlx::query!(
        r#"
        SELECT uid, password
        FROM users
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

#[tracing::instrument(
    name = "Validate credentials",
    skip(expected_password_hash, password_candidate)
)]
fn verify_password_hash(
    expected_password_hash: Secret<String>,
    password_candidate: Secret<String>,
) -> Result<(), anyhow::Error> {
    let expected_hash_password = PasswordHash::new(expected_password_hash.expose_secret())
        .context("Failed to parse hash in PHC string format.")?;

    Argon2::default()
        .verify_password(
            password_candidate.expose_secret().as_bytes(),
            &expected_hash_password,
        )
        .context("Invalid password.")
}
