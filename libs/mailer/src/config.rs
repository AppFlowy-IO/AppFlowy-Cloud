use secrecy::Secret;

#[derive(serde::Deserialize, Clone, Debug)]
pub struct MailerSetting {
  pub smtp_host: String,
  pub smtp_port: u16,
  pub smtp_username: String,
  pub smtp_password: Secret<String>,
}
