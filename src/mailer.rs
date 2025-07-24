use mailer::sender::Mailer;
use std::collections::HashMap;

pub const WORKSPACE_INVITE_TEMPLATE_NAME: &str = "workspace_invite";
pub const WORKSPACE_ACCESS_REQUEST_TEMPLATE_NAME: &str = "workspace_access_request";
pub const WORKSPACE_ACCESS_REQUEST_APPROVED_NOTIFICATION_TEMPLATE_NAME: &str =
  "workspace_access_request_approved_notification";
pub const PAGE_MENTION_NOTIFICATION_TEMPLATE_NAME: &str = "page_mention_notification";

#[derive(Clone)]
pub struct AFCloudMailer(Mailer);
impl AFCloudMailer {
  pub async fn new(mut mailer: Mailer) -> Result<Self, anyhow::Error> {
    register_mailer(&mut mailer).await?;
    Ok(Self(mailer))
  }

  pub async fn send_workspace_invite(
    &self,
    email: &str,
    param: WorkspaceInviteMailerParam,
  ) -> Result<(), anyhow::Error> {
    let subject = format!(
      "{} invited you to {} in AppFlowy",
      param.username, param.workspace_name
    );
    self
      .0
      .send_email_template(
        Some(param.username.clone()),
        email,
        WORKSPACE_INVITE_TEMPLATE_NAME,
        param,
        &subject,
      )
      .await
      .map(|_| tracing::info!("Sent workspace invite email to {}", email))
      .map_err(|err| {
        tracing::error!(
          "Failed to send workspace invite email to {}: {}",
          email,
          err
        );
        err
      })
  }

  pub async fn send_workspace_access_request(
    &self,
    recipient_name: &str,
    email: &str,
    param: WorkspaceAccessRequestMailerParam,
  ) -> Result<(), anyhow::Error> {
    let subject = format!(
      "{} requested access to {} in AppFlowy",
      param.username, param.workspace_name
    );
    self
      .0
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
      .0
      .send_email_template(
        Some(recipient_name.to_string()),
        email,
        WORKSPACE_ACCESS_REQUEST_APPROVED_NOTIFICATION_TEMPLATE_NAME,
        param,
        subject,
      )
      .await
  }

  pub async fn send_page_mention_notification(
    &self,
    recipient_name: &str,
    email: &str,
    param: &PageMentionNotificationMailerParam,
  ) -> Result<(), anyhow::Error> {
    let subject = format!(
      "{} has mentioned you in {} in AppFlowy",
      param.mentioner_name, param.mentioned_page_name
    );
    self
      .0
      .send_email_template(
        Some(recipient_name.to_string()),
        email,
        PAGE_MENTION_NOTIFICATION_TEMPLATE_NAME,
        param,
        &subject,
      )
      .await
  }
}

async fn register_mailer(mailer: &mut Mailer) -> Result<(), anyhow::Error> {
  let workspace_invite_template =
    include_str!("../assets/mailer_templates/build_production/workspace_invitation.html");
  let access_request_template =
    include_str!("../assets/mailer_templates/build_production/access_request.html");
  let access_request_approved_notification_template = include_str!(
    "../assets/mailer_templates/build_production/access_request_approved_notification.html"
  );
  let page_mention_notification_template =
    include_str!("../assets/mailer_templates/build_production/page_mention_notification.html");
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
    (
      PAGE_MENTION_NOTIFICATION_TEMPLATE_NAME,
      page_mention_notification_template,
    ),
  ]);

  for (template_name, template_string) in template_strings {
    mailer
      .register_template(template_name, template_string)
      .await
      .map_err(|err| anyhow::anyhow!(format!("Failed to register handlebars template: {}", err)))?;
  }

  Ok(())
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

#[derive(serde::Serialize)]
pub struct PageMentionNotificationMailerParam {
  pub workspace_name: String,
  pub mentioned_page_name: String,
  pub mentioner_icon_url: String,
  pub mentioner_name: String,
  pub mentioned_page_url: String,
  pub mentioned_at: String,
}
