use mailer::sender::Mailer;
use std::ops::Deref;

pub const IMPORT_NOTION_TEMPLATE_NAME: &str = "import_notion_report";
#[derive(Clone)]
pub struct AFWorkerMailer(Mailer);

impl Deref for AFWorkerMailer {
  type Target = Mailer;

  fn deref(&self) -> &Self::Target {
    &self.0
  }
}

impl AFWorkerMailer {
  pub async fn new(mut mailer: Mailer) -> Result<Self, anyhow::Error> {
    let import_notion_report =
      include_str!("../../../assets/mailer_templates/build_production/import_notion.html");

    for (name, template) in [(IMPORT_NOTION_TEMPLATE_NAME, import_notion_report)] {
      mailer
        .register_template(name, template)
        .await
        .map_err(|err| {
          anyhow::anyhow!(format!("Failed to register handlebars template: {}", err))
        })?;
    }

    Ok(Self(mailer))
  }

  pub async fn send_import_notion_report(
    &self,
    recipient_name: &str,
    email: &str,
    param: ImportNotionReportMailerParam,
  ) -> Result<(), anyhow::Error> {
    let subject = "Notification: Import Notion";
    self
      .0
      .send_email_template(
        Some(recipient_name.to_string()),
        email,
        IMPORT_NOTION_TEMPLATE_NAME,
        param,
        subject,
      )
      .await
  }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ImportNotionReportMailerParam {
  pub user_name: String,
  pub file_name: String,
  pub workspace_id: String,
  pub workspace_name: String,
  pub open_workspace: bool,
  pub error: Option<String>,
  pub error_detail: Option<String>,
}

#[cfg(test)]
mod tests {
  use crate::mailer::{AFWorkerMailer, ImportNotionReportMailerParam, IMPORT_NOTION_TEMPLATE_NAME};
  use mailer::sender::Mailer;

  #[tokio::test]
  async fn render_import_report() {
    let mailer = Mailer::new(
      "test mailer".to_string(),
      "123".to_string(),
      "localhost",
      465,
    )
    .await
    .unwrap();
    let worker_mailer = AFWorkerMailer::new(mailer).await.unwrap();
    let s = worker_mailer
      .render(
        IMPORT_NOTION_TEMPLATE_NAME,
        &ImportNotionReportMailerParam {
          user_name: "nathan".to_string(),
          file_name: "working".to_string(),
          workspace_id: "1".to_string(),
          workspace_name: "working".to_string(),
          open_workspace: true,
          error: None,
          error_detail: None,
        },
      )
      .unwrap();

    println!("{}", s);
  }
}
