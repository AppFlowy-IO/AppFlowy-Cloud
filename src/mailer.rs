use lettre::message::header::ContentType;
use lettre::message::Message;
use lettre::transport::smtp::authentication::Credentials;
use lettre::Address;
use lettre::AsyncSmtpTransport;
use lettre::AsyncTransport;
use std::sync::Arc;
use std::sync::RwLock;

lazy_static::lazy_static! {
    static ref HANDLEBARS: Arc<RwLock<handlebars::Handlebars<'static>>> =
        Arc::new(handlebars::Handlebars::new().into());
}

#[derive(Clone)]
pub struct Mailer {
  smtp_transport: AsyncSmtpTransport<lettre::Tokio1Executor>,
  smtp_username: String,
}

impl Mailer {
  pub async fn new(
    smtp_username: String,
    smtp_password: String,
    smtp_host: &str,
    smtp_port: u16,
  ) -> Result<Self, anyhow::Error> {
    let creds = Credentials::new(smtp_username.clone(), smtp_password);
    let smtp_transport = AsyncSmtpTransport::<lettre::Tokio1Executor>::relay(smtp_host)?
      .credentials(creds)
      .port(smtp_port)
      .build();

    let workspace_invite_template =
      include_str!("../assets/mailer_templates/build_production/workspace_invitation.html");

    HANDLEBARS
      .write()
      .unwrap()
      .register_template_string("workspace_invite", workspace_invite_template)
      .unwrap();

    Ok(Self {
      smtp_transport,
      smtp_username,
    })
  }

  pub async fn send_workspace_invite(
    &self,
    email: String,
    param: WorkspaceInviteMailerParam,
  ) -> Result<(), anyhow::Error> {
    let rendered = HANDLEBARS
      .read()
      .unwrap()
      .render("workspace_invite", &param)?;

    let email = Message::builder()
      .from(lettre::message::Mailbox::new(
        Some("AppFlowy Notification".to_string()),
        self.smtp_username.parse::<Address>()?,
      ))
      .to(lettre::message::Mailbox::new(
        Some(param.username.clone()),
        email.parse().unwrap(),
      ))
      .subject(format!(
        "Action required: {} invited you to {} in AppFlowy",
        param.username, param.workspace_name
      ))
      .header(ContentType::TEXT_HTML)
      .body(rendered)?;

    AsyncTransport::send(&self.smtp_transport, email).await?;
    Ok(())
  }
}

#[derive(serde::Serialize)]
pub struct WorkspaceInviteMailerParam {
  pub user_icon_url: String,
  pub username: String, // Inviter
  pub workspace_name: String,
  pub workspace_icon_url: String,
  pub workspace_member_count: String,
  pub accept_url: String,
}
