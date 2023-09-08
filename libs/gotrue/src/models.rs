use serde::{Deserialize, Serialize};

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
  pub expires_at: Option<i64>, // older versions of GoTrue do not return this
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

#[derive(Serialize, Deserialize, Debug)]
pub struct GoTrueError {
  pub code: i64,
  pub msg: String,
  pub error_id: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct GoTrueSettings {
  pub external: GoTrueProviderSettings,
  pub disable_signup: bool,
  pub mailer_autoconfirm: bool,
  pub phone_autoconfirm: bool,
  pub sms_provider: String,
  pub mfa_enabled: Option<bool>, // older versions of GoTrue do not return this
  pub saml_enabled: Option<bool>, // older versions of GoTrue do not return this
}

#[derive(Serialize, Deserialize, Debug)]
pub struct GoTrueProviderSettings {
  pub apple: bool,
  pub azure: bool,
  pub bitbucket: bool,
  pub discord: bool,
  pub facebook: bool,
  pub figma: Option<bool>, // older versions of GoTrue do not return this
  pub github: bool,
  pub gitlab: bool,
  pub google: bool,
  pub keycloak: bool,
  pub kakao: Option<bool>, // older versions of GoTrue do not return this
  pub linkedin: bool,
  pub notion: bool,
  pub spotify: bool,
  pub slack: bool,
  pub workos: bool,
  pub twitch: bool,
  pub twitter: bool,
  pub email: bool,
  pub phone: bool,
  pub zoom: bool,
}
