use crate::import_worker::report::{ImportNotifier, ImportProgress};
use crate::mailer::{AFWorkerMailer, IMPORT_NOTION_TEMPLATE_NAME};
use axum::async_trait;
use tracing::{error, info};

pub struct EmailNotifier(AFWorkerMailer);
impl EmailNotifier {
  pub fn new(mailer: AFWorkerMailer) -> Self {
    Self(mailer)
  }
}

#[async_trait]
impl ImportNotifier for EmailNotifier {
  async fn notify_progress(&self, progress: ImportProgress) {
    match progress {
      ImportProgress::Started { workspace_id: _ } => {},
      ImportProgress::Finished(result) => {
        let subject = "Notification: Import Report";
        if let Some(value) = result.value {
          info!(
            "[Import]: sending import notion report email to {}",
            result.user_email
          );
          if let Err(err) = self
            .0
            .send_email_template(
              Some(result.user_name),
              &result.user_email,
              IMPORT_NOTION_TEMPLATE_NAME,
              value,
              subject,
            )
            .await
          {
            error!("Failed to send import notion report email: {}", err);
          }
        }
      },
    }
  }
}
