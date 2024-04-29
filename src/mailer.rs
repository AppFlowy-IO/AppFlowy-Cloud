use lettre::message::header::ContentType;
use lettre::message::Message;
use lettre::transport::smtp::authentication::Credentials;
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
}

impl Mailer {
  pub async fn new(
    smtp_username: String,
    smtp_password: String,
    smtp_host: &str,
    workspace_invite_template_url: &str,
  ) -> Result<Self, anyhow::Error> {
    let creds = Credentials::new(smtp_username, smtp_password);
    let smtp_transport = AsyncSmtpTransport::<lettre::Tokio1Executor>::relay(smtp_host)?
      .credentials(creds)
      .build();

    let http_client = reqwest::Client::new();
    let workspace_invite_template = http_client
      .get(workspace_invite_template_url)
      .send()
      .await?
      .error_for_status()?
      .text()
      .await?;

    HANDLEBARS
      .write()
      .unwrap()
      .register_template_string("workspace_invite", &workspace_invite_template)
      .unwrap();

    Ok(Self { smtp_transport })
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
        Some("AppFlowy Notify".to_string()),
        lettre::Address::new("notify", "appflowy.io")?,
      ))
      .to(lettre::message::Mailbox::new(
        Some(param.username),
        email.parse().unwrap(),
      ))
      .subject("AppFlowy Workpace Invitation")
      .header(ContentType::TEXT_HTML)
      .body(rendered)?;

    AsyncTransport::send(&self.smtp_transport, email).await?;
    Ok(())
  }
}

#[derive(serde::Serialize)]
pub struct WorkspaceInviteMailerParam {
  pub user_icon_url: String,
  pub username: String,
  pub workspace_name: String,
  pub workspace_icon_url: String,
  pub workspace_member_count: String,
  pub accept_url: String,
}
