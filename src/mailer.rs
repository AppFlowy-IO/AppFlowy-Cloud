use lettre::message::header::ContentType;
use lettre::message::Message;
use lettre::transport::smtp::authentication::Credentials;
use lettre::Address;
use lettre::AsyncSmtpTransport;
use lettre::AsyncTransport;
use std::collections::HashMap;
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

pub const WORKSPACE_INVITE_TEMPLATE_NAME: &str = "workspace_invite";
pub const WORKSPACE_ACCESS_REQUEST_TEMPLATE_NAME: &str = "workspace_access_request";
pub const WORKSPACE_ACCESS_REQUEST_APPROVED_NOTIFICATION_TEMPLATE_NAME: &str =
  "workspace_access_request_approved_notification";

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
    let access_request_template =
      include_str!("../assets/mailer_templates/build_production/access_request.html");
    let access_request_approved_notification_template = include_str!(
      "../assets/mailer_templates/build_production/access_request_approved_notification.html"
    );
    let template_strings = HashMap::from([
      (WORKSPACE_INVITE_TEMPLATE_NAME, workspace_invite_template),
      (
        WORKSPACE_ACCESS_REQUEST_TEMPLATE_NAME,
        access_request_template,
      ),
      (
        WORKSPACE_ACCESS_REQUEST_APPROVED_NOTIFICATION_TEMPLATE_NAME,
        access_request_approved_notification_template,
      ),
    ]);

    for (template_name, template_string) in template_strings {
      HANDLEBARS
        .write()
        .map_err(|err| anyhow::anyhow!(format!("Failed to write handlebars: {}", err)))?
        .register_template_string(template_name, template_string)
        .map_err(|err| {
          anyhow::anyhow!(format!("Failed to register handlebars template: {}", err))
        })?;
    }

    Ok(Self {
      smtp_transport,
      smtp_username,
    })
  }

  async fn send_email_template<T>(
    &self,
    recipient_name: Option<String>,
    email: &str,
    template_name: &str,
    param: T,
    subject: &str,
  ) -> Result<(), anyhow::Error>
  where
    T: serde::Serialize,
  {
    let rendered = match HANDLEBARS.read() {
      Ok(registory) => registory.render(template_name, &param)?,
      Err(err) => anyhow::bail!(format!("Failed to render handlebars template: {}", err)),
    };

    let email = Message::builder()
      .from(lettre::message::Mailbox::new(
        Some("AppFlowy Notification".to_string()),
        self.smtp_username.parse::<Address>()?,
      ))
      .to(lettre::message::Mailbox::new(
        recipient_name,
        email.parse()?,
      ))
      .subject(subject)
      .header(ContentType::TEXT_HTML)
      .body(rendered)?;

    AsyncTransport::send(&self.smtp_transport, email).await?;
    Ok(())
  }
  pub async fn send_workspace_invite(
    &self,
    email: &str,
    param: WorkspaceInviteMailerParam,
  ) -> Result<(), anyhow::Error> {
    let subject = format!(
      "Action required: {} invited you to {} in AppFlowy",
      param.username, param.workspace_name
    );
    self
      .send_email_template(
        Some(param.username.clone()),
        email,
        WORKSPACE_INVITE_TEMPLATE_NAME,
        param,
        &subject,
      )
      .await
  }

  pub async fn send_workspace_access_request(
    &self,
    recipient_name: &str,
    email: &str,
    param: WorkspaceAccessRequestMailerParam,
  ) -> Result<(), anyhow::Error> {
    let subject = format!(
      "Action required: {} requested access to {} in AppFlowy",
      param.username, param.workspace_name
    );
    self
      .send_email_template(
        Some(recipient_name.to_string()),
        email,
        WORKSPACE_ACCESS_REQUEST_TEMPLATE_NAME,
        param,
        &subject,
      )
      .await
  }

  pub async fn send_workspace_access_request_approval_notification(
    &self,
    recipient_name: &str,
    email: &str,
    param: WorkspaceAccessRequestApprovedMailerParam,
  ) -> Result<(), anyhow::Error> {
    let subject = "Notification: Workspace access request approved";
    self
      .send_email_template(
        Some(recipient_name.to_string()),
        email,
        WORKSPACE_ACCESS_REQUEST_APPROVED_NOTIFICATION_TEMPLATE_NAME,
        param,
        subject,
      )
      .await
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

#[derive(serde::Serialize)]
pub struct WorkspaceAccessRequestMailerParam {
  pub user_icon_url: String,
  pub username: String,
  pub workspace_name: String,
  pub workspace_icon_url: String,
  pub workspace_member_count: i64,
  pub approve_url: String,
}

#[derive(serde::Serialize)]
pub struct WorkspaceAccessRequestApprovedMailerParam {
  pub workspace_name: String,
  pub workspace_icon_url: String,
  pub workspace_member_count: i64,
  pub launch_workspace_url: String,
}
