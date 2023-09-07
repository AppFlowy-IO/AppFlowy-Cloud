use gotrue::api::Client;
use server_error::Error;

pub async fn sign_up(gotrue_client: &Client, email: &str, password: &str) -> Result<(), Error> {
  gotrue_client.sign_up(&email, &password).await??;

  // TODO: set up workspace for new user

  Ok(())
}
