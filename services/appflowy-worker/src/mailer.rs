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
      include_str!("../../../assets/mailer_templates/build_production/import_notion_report.html");

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
    let subject = "Notification: Import Notion report";
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
}
