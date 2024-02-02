use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt::{Display, Formatter};

#[derive(Debug, Serialize, Deserialize, Clone)]
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
pub struct AdminListUsersResponse {
  pub users: Vec<User>,
  pub aud: String,
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
  pub identities: Option<Vec<Identity>>,

  pub created_at: String,
  pub updated_at: String,
  pub banned_until: Option<String>,
  pub deleted_at: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Factor {
  pub id: String,
  pub created_at: String,
  pub updated_at: String,
  pub status: String,
  pub friendly_name: Option<String>,
  pub factor_type: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct GotrueTokenResponse {
  /// the token that clients use to make authenticated requests to the server or API. It is a bearer token that provides temporary, secure access to server resources.
  pub access_token: String,
  pub token_type: String,
  /// the access_token will remain valid before it expires and needs to be refreshed.
  pub expires_in: i64,
  /// a timestamp in seconds indicating the exact time at which the access_token will expire.
  pub expires_at: i64,
  /// The refresh token is used to obtain a new access_token once the current access_token expires.
  /// Refresh tokens are usually long-lived and are stored securely by the client.
  pub refresh_token: String,
  pub user: User,
  pub provider_access_token: Option<String>,
  pub provider_refresh_token: Option<String>,
}

impl Display for GotrueTokenResponse {
  fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("GotrueTokenResponse")
      .field("expires_at", &self.expires_at)
      .field("token_type", &self.token_type)
      .finish()
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
  pub fn has_provider(&self, p: &AuthProvider) -> bool {
    let a = self.0.get(p.as_str());
    match a {
      Some(v) => *v,
      None => false,
    }
  }

  pub fn oauth_providers(&self) -> Vec<&str> {
    self
      .0
      .iter()
      .filter(|&(key, &value)| value && key != "email" && key != "phone")
      .map(|(key, _value)| key.as_str())
      .collect()
  }
}

pub enum AuthProvider {
  // Non-OAuth providers
  Email,
  Phone,

  // OAuth providers
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
  Zoom,
}

impl AuthProvider {
  pub fn as_str(&self) -> &str {
    match self {
      AuthProvider::Apple => "apple",
      AuthProvider::Azure => "azure",
      AuthProvider::Bitbucket => "bitbucket",
      AuthProvider::Discord => "discord",
      AuthProvider::Facebook => "facebook",
      AuthProvider::Figma => "figma",
      AuthProvider::Github => "github",
      AuthProvider::Gitlab => "gitlab",
      AuthProvider::Google => "google",
      AuthProvider::Keycloak => "keycloak",
      AuthProvider::Kakao => "kakao",
      AuthProvider::Linkedin => "linkedin",
      AuthProvider::Notion => "notion",
      AuthProvider::Spotify => "spotify",
      AuthProvider::Slack => "slack",
      AuthProvider::Workos => "workos",
      AuthProvider::Twitch => "twitch",
      AuthProvider::Twitter => "twitter",
      AuthProvider::Email => "email",
      AuthProvider::Phone => "phone",
      AuthProvider::Zoom => "zoom",
    }
  }
}

impl AuthProvider {
  pub fn from<A: AsRef<str>>(value: A) -> Option<AuthProvider> {
    match value.as_ref() {
      "apple" => Some(AuthProvider::Apple),
      "azure" => Some(AuthProvider::Azure),
      "bitbucket" => Some(AuthProvider::Bitbucket),
      "discord" => Some(AuthProvider::Discord),
      "facebook" => Some(AuthProvider::Facebook),
      "figma" => Some(AuthProvider::Figma),
      "github" => Some(AuthProvider::Github),
      "gitlab" => Some(AuthProvider::Gitlab),
      "google" => Some(AuthProvider::Google),
      "keycloak" => Some(AuthProvider::Keycloak),
      "kakao" => Some(AuthProvider::Kakao),
      "linkedin" => Some(AuthProvider::Linkedin),
      "notion" => Some(AuthProvider::Notion),
      "spotify" => Some(AuthProvider::Spotify),
      "slack" => Some(AuthProvider::Slack),
      "workos" => Some(AuthProvider::Workos),
      "twitch" => Some(AuthProvider::Twitch),
      "twitter" => Some(AuthProvider::Twitter),
      "email" => Some(AuthProvider::Email),
      "phone" => Some(AuthProvider::Phone),
      "zoom" => Some(AuthProvider::Zoom),
      _ => None,
    }
  }
}

#[derive(Serialize, Deserialize)]
pub struct OAuthURL {
  pub url: String,
}
#[derive(Serialize, Deserialize, Debug)]
#[serde(untagged)]
pub enum SignUpResponse {
  Authenticated(GotrueTokenResponse),
  NotAuthenticated(User),
}
#[derive(Default, Serialize, Deserialize)]
pub struct UpdateGotrueUserParams {
  pub email: String,
  pub password: Option<String>,
  pub nonce: String,
  pub data: BTreeMap<String, serde_json::Value>,
  pub app_metadata: Option<BTreeMap<String, serde_json::Value>>,
  pub phone: String,
  pub channel: String,
  pub code_challenge: String,
  pub code_challenge_method: String,
}

impl UpdateGotrueUserParams {
  pub fn new() -> Self {
    Self::default()
  }

  pub fn with_opt_email<T: ToString>(mut self, email: Option<T>) -> Self {
    self.email = email.map(|v| v.to_string()).unwrap_or_default();
    self
  }

  pub fn with_opt_password<T: ToString>(mut self, password: Option<T>) -> Self {
    self.password = password.map(|v| v.to_string());
    self
  }
}
