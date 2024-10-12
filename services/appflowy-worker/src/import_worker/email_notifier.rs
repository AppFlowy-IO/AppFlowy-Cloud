use crate::import_worker::report::{ImportNotifier, ImportProgress};
use crate::mailer::{AFWorkerMailer, IMPORT_FAIL_TEMPLATE, IMPORT_SUCCESS_TEMPLATE};
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
        info!(
          "[Import]: sending import notion report email to {}",
          result.user_email
        );

        let template_name = if result.is_success {
          IMPORT_SUCCESS_TEMPLATE
        } else {
          IMPORT_FAIL_TEMPLATE
        };

        if let Err(err) = self
          .0
          .send_email_template(
            Some(result.user_name),
            &result.user_email,
            template_name,
            result.value,
            subject,
          )
          .await
        {
          error!("Failed to send import notion report email: {}", err);
        }
      },
    }
  }
}
