use crate::Client;
use gotrue::grant::{Grant, PasswordGrant};
use gotrue::params::GenerateLinkParams;
use gotrue_entity::GoTrueError;
use scraper::{Html, Selector};

impl Client {
  pub async fn generate_sign_in_url(
    &self,
    admin_user_email: &str,
    admin_user_password: &str,
    user_email: &str,
  ) -> Result<String, GoTrueError> {
    let admin_token = self
      .gotrue_client
      .token(&Grant::Password(PasswordGrant {
        email: admin_user_email.to_string(),
        password: admin_user_password.to_string(),
      }))
      .await?;

    let admin_user_params: GenerateLinkParams = GenerateLinkParams {
      email: user_email.to_string(),
      ..Default::default()
    };

    let link_resp = self
      .gotrue_client
      .generate_link(&admin_token.access_token, &admin_user_params)
      .await?;
    assert_eq!(link_resp.email, user_email);

    let action_link = link_resp.action_link;
    let resp = reqwest::Client::new().get(action_link).send().await?;
    let resp_text = resp.text().await?;
    Ok(extract_appflowy_sign_in_url(&resp_text))
  }
}

pub fn extract_appflowy_sign_in_url(html_str: &str) -> String {
  let fragment = Html::parse_fragment(html_str);
  let selector = Selector::parse("a").unwrap();
  fragment
    .select(&selector)
    .next()
    .unwrap()
    .value()
    .attr("href")
    .unwrap()
    .to_string()
}
