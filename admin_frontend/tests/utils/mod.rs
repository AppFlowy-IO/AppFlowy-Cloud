pub mod test_config;

use admin_frontend::{
  config::Config,
  models::{OAuthRedirect, OAuthRedirectToken, WebApiLoginRequest},
};
use test_config::TestConfig;

pub struct AdminFrontendClient {
  test_config: TestConfig,
  #[allow(dead_code)]
  server_config: Config,
  session_id: Option<String>,
  http_client: reqwest::Client,
}

impl AdminFrontendClient {
  pub fn new() -> Self {
    dotenvy::dotenv().ok();

    let server_config = Config::from_env().unwrap();
    let test_config = TestConfig::from_env();
    let http_client = reqwest::Client::new();
    Self {
      server_config,
      session_id: None,
      http_client,
      test_config,
    }
  }

  pub async fn web_api_sign_in(&mut self, email: &str, password: &str) {
    let url = format!(
      "{}{}/web-api/signin",
      self.test_config.hostname, self.server_config.path_prefix
    );
    let resp = self
      .http_client
      .post(&url)
      .form(&WebApiLoginRequest {
        email: email.to_string(),
        password: password.to_string(),
        redirect_to: None,
      })
      .send()
      .await
      .unwrap();
    let resp = check_resp(resp).await;
    let c = resp.cookies().find(|c| c.name() == "session_id").unwrap();
    self.session_id = Some(c.value().to_string());
  }

  pub async fn web_api_oauth_redirect(
    &mut self,
    oauth_redirect: &OAuthRedirect,
  ) -> reqwest::Response {
    let url = format!(
      "{}{}/web-api/oauth-redirect",
      self.test_config.hostname, self.server_config.path_prefix
    );
    let http_client = reqwest::Client::builder()
      .redirect(reqwest::redirect::Policy::none())
      .build()
      .unwrap();

    http_client
      .get(&url)
      .header("Cookie", format!("session_id={}", self.session_id()))
      .query(oauth_redirect)
      .send()
      .await
      .unwrap()
  }

  pub async fn web_api_oauth_redirect_token(
    &mut self,
    oauth_redirect: &OAuthRedirectToken,
  ) -> reqwest::Response {
    let url = format!(
      "{}{}/web-api/oauth-redirect/token",
      self.test_config.hostname, self.server_config.path_prefix
    );
    self
      .http_client
      .get(&url)
      .header("Cookie", format!("session_id={}", self.session_id()))
      .query(oauth_redirect)
      .send()
      .await
      .unwrap()
  }

  fn session_id(&self) -> &str {
    self.session_id.as_ref().unwrap()
  }
}

async fn check_resp(resp: reqwest::Response) -> reqwest::Response {
  if resp.status() != 200 {
    println!("resp: {:#?}", resp);
    let payload = resp.text().await.unwrap();
    panic!("payload: {:#?}", payload)
  }
  resp
}
