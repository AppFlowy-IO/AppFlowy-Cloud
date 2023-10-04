use serde::{Deserialize, Serialize};
use std::fmt::Formatter;
use std::{collections::BTreeMap, fmt::Display};

#[derive(Debug, Serialize, Deserialize)]
pub struct Identity {
  pub id: String,
  pub user_id: String,
  pub identity_data: Option<serde_json::Value>,
  pub provider: String,
  pub last_sign_in_at: String,
  pub created_at: String,
  pub updated_at: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct User {
  pub id: String,

  pub aud: String,
  pub role: String,
  pub email: String,

  pub email_confirmed_at: Option<String>,
  pub invited_at: Option<String>,

  pub phone: String,
  pub phone_confirmed_at: Option<String>,

  pub confirmation_sent_at: Option<String>,

  // For backward compatibility only. Use EmailConfirmedAt or PhoneConfirmedAt instead.
  pub confirmed_at: Option<String>,

  pub recovery_sent_at: Option<String>,

  pub new_email: Option<String>,
  pub email_change_sent_at: Option<String>,

  pub new_phone: Option<String>,
  pub phone_change_sent_at: Option<String>,

  pub reauthentication_sent_at: Option<String>,

  pub last_sign_in_at: Option<String>,

  pub app_metadata: serde_json::Value,
  pub user_metadata: serde_json::Value,

  pub factors: Option<Vec<Factor>>,
  pub identities: Vec<Identity>,

  pub created_at: String,
  pub updated_at: String,
  pub banned_until: Option<String>,
  pub deleted_at: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Factor {
  pub id: String,
  pub created_at: String,
  pub updated_at: String,
  pub status: String,
  pub friendly_name: Option<String>,
  pub factor_type: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct AccessTokenResponse {
  pub access_token: String,
  pub token_type: String,
  pub expires_in: i64,
  pub expires_at: i64,
  pub refresh_token: String,
  pub user: User,
  pub provider_access_token: Option<String>,
  pub provider_refresh_token: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct OAuthError {
  pub error: String,
  pub error_description: Option<String>,
}

impl Display for OAuthError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self.error_description {
      Some(ref desc) => write!(f, "{}: {}", self.error, desc),
      None => write!(f, "{}", self.error),
    }
  }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct GoTrueError {
  pub code: i64,
  pub msg: String,
  pub error_id: Option<String>,
}

impl Display for GoTrueError {
  fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
    f.write_fmt(format_args!(
      "gotrue error: {} code: {}, error_id: {:?}",
      self.msg, self.code, self.error_id
    ))
  }
}

impl From<anyhow::Error> for GoTrueError {
  fn from(value: anyhow::Error) -> Self {
    GoTrueError {
      code: -1,
      msg: format!("gotrue unhandled error: {}", value),
      error_id: None,
    }
  }
}

impl From<reqwest::Error> for GoTrueError {
  fn from(value: reqwest::Error) -> Self {
    GoTrueError {
      code: -1,
      msg: format!("gotrue reqwest error: {}", value),
      error_id: None,
    }
  }
}

impl From<OAuthError> for GoTrueError {
  fn from(value: OAuthError) -> Self {
    GoTrueError {
      code: 400,
      msg: format!(
        "oauth error: {}, description: {}",
        value.error,
        value.error_description.unwrap_or_default(),
      ),
      error_id: None,
    }
  }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct GoTrueSettings {
  pub external: GoTrueOAuthProviderSettings,
  pub disable_signup: bool,
  pub mailer_autoconfirm: bool,
  pub phone_autoconfirm: bool,
  pub sms_provider: String,
  pub mfa_enabled: bool,
  pub saml_enabled: bool,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct GoTrueOAuthProviderSettings(BTreeMap<String, bool>);

impl GoTrueOAuthProviderSettings {
  pub fn has_provider(&self, p: &OAuthProvider) -> bool {
    let a = self.0.get(p.as_str());
    match a {
      Some(v) => *v,
      None => false,
    }
  }
}

pub enum OAuthProvider {
  Apple,
  Azure,
  Bitbucket,
  Discord,
  Facebook,
  Figma,
  Github,
  Gitlab,
  Google,
  Keycloak,
  Kakao,
  Linkedin,
  Notion,
  Spotify,
  Slack,
  Workos,
  Twitch,
  Twitter,
  Email,
  Phone,
  Zoom,
}

impl OAuthProvider {
  pub fn as_str(&self) -> &str {
    match self {
      OAuthProvider::Apple => "apple",
      OAuthProvider::Azure => "azure",
      OAuthProvider::Bitbucket => "bitbucket",
      OAuthProvider::Discord => "discord",
      OAuthProvider::Facebook => "facebook",
      OAuthProvider::Figma => "figma",
      OAuthProvider::Github => "github",
      OAuthProvider::Gitlab => "gitlab",
      OAuthProvider::Google => "google",
      OAuthProvider::Keycloak => "keycloak",
      OAuthProvider::Kakao => "kakao",
      OAuthProvider::Linkedin => "linkedin",
      OAuthProvider::Notion => "notion",
      OAuthProvider::Spotify => "spotify",
      OAuthProvider::Slack => "slack",
      OAuthProvider::Workos => "workos",
      OAuthProvider::Twitch => "twitch",
      OAuthProvider::Twitter => "twitter",
      OAuthProvider::Email => "email",
      OAuthProvider::Phone => "phone",
      OAuthProvider::Zoom => "zoom",
    }
  }
}

impl OAuthProvider {
  pub fn from<A: AsRef<str>>(value: A) -> Option<OAuthProvider> {
    match value.as_ref() {
      "apple" => Some(OAuthProvider::Apple),
      "azure" => Some(OAuthProvider::Azure),
      "bitbucket" => Some(OAuthProvider::Bitbucket),
      "discord" => Some(OAuthProvider::Discord),
      "facebook" => Some(OAuthProvider::Facebook),
      "figma" => Some(OAuthProvider::Figma),
      "github" => Some(OAuthProvider::Github),
      "gitlab" => Some(OAuthProvider::Gitlab),
      "google" => Some(OAuthProvider::Google),
      "keycloak" => Some(OAuthProvider::Keycloak),
      "kakao" => Some(OAuthProvider::Kakao),
      "linkedin" => Some(OAuthProvider::Linkedin),
      "notion" => Some(OAuthProvider::Notion),
      "spotify" => Some(OAuthProvider::Spotify),
      "slack" => Some(OAuthProvider::Slack),
      "workos" => Some(OAuthProvider::Workos),
      "twitch" => Some(OAuthProvider::Twitch),
      "twitter" => Some(OAuthProvider::Twitter),
      "email" => Some(OAuthProvider::Email),
      "phone" => Some(OAuthProvider::Phone),
      "zoom" => Some(OAuthProvider::Zoom),
      _ => None,
    }
  }
}

#[derive(Serialize, Deserialize)]
pub struct OAuthURL {
  pub url: String,
}

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
pub enum SignUpResponse {
  Authenticated(AccessTokenResponse),
  NotAuthenticated(User),
}
