use anyhow::Error;

use crate::{
  component::auth::gotrue::models::{AccessTokenResponse, OAuthError, TokenResult, User},
  utils::http_response::from_response,
};

pub struct Client {
  http_client: reqwest::Client,
  base_url: String,
  token: Option<AccessTokenResponse>,
}

impl Client {
  pub fn from(c: reqwest::Client, base_url: &str) -> Self {
    Self {
      base_url: base_url.to_string(),
      http_client: c,
      token: None,
    }
  }

  pub fn token(&self) -> Option<&AccessTokenResponse> {
    self.token.as_ref()
  }

  pub async fn sign_in_password(
    &mut self,
    email: &str,
    password: &str,
  ) -> Result<Result<(), OAuthError>, Error> {
    let url = format!("{}/api/user/sign_in/password", self.base_url);
    let payload = serde_json::json!({
        "email": email,
        "password": password,
    });
    let resp = self.http_client.post(&url).json(&payload).send().await?;
    let token_result: TokenResult = from_response(resp).await?;
    match token_result {
      TokenResult::Success(s) => {
        self.token = Some(s);
        Ok(Ok(()))
      },
      TokenResult::Fail(e) => Ok(Err(e)),
    }
  }

  pub async fn sign_up(&self, email: &str, password: &str) -> Result<User, Error> {
    let url = format!("{}/api/user/sign_up", self.base_url);
    let payload = serde_json::json!({
        "email": email,
        "password": password,
    });
    let resp = self.http_client.post(&url).json(&payload).send().await?;
    from_response(resp).await
  }

  pub async fn sign_out(&self, email: &str, password: &str) -> Result<User, Error> {
    let url = format!("{}/api/user/sign_out", self.base_url);
    let payload = serde_json::json!({
        "email": email,
        "password": password,
    });
    let resp = self.http_client.post(&url).json(&payload).send().await?;
    from_response(resp).await
  }

  // pub async fn change_password(
  //   &self,
  //   current_password: &str,
  //   new_password: &str,
  //   new_password_confirm: &str,
  // ) -> Result<(), Error> {
  //   let auth_token = match &self.token_old {
  //     Some(t) => t.to_string(),
  //     None => anyhow::bail!("no token found, are you logged in?"),
  //   };

  //   let url = format!("{}/api/user/password", self.base_url);
  //   let payload = serde_json::json!({
  //       "current_password": current_password,
  //       "new_password": new_password,
  //       "new_password_confirm": new_password_confirm,
  //   });
  //   let resp = self
  //     .http_client
  //     .post(&url)
  //     .header(HEADER_TOKEN, auth_token)
  //     .json(&payload)
  //     .send()
  //     .await?;
  //   check_response(resp).await
  // }
}
