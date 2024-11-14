use mailer::sender::Mailer;
use std::ops::Deref;

pub const IMPORT_SUCCESS_TEMPLATE: &str = "import_notion_success";
pub const IMPORT_FAIL_TEMPLATE: &str = "import_notion_fail";
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
    let import_data_success =
      include_str!("../../../assets/mailer_templates/build_production/import_data_success.html");

    let import_data_fail =
      include_str!("../../../assets/mailer_templates/build_production/import_data_fail.html");

    for (name, template) in [
      (IMPORT_SUCCESS_TEMPLATE, import_data_success),
      (IMPORT_FAIL_TEMPLATE, import_data_fail),
    ] {
      mailer
        .register_template(name, template)
        .await
        .map_err(|err| {
          anyhow::anyhow!(format!("Failed to register handlebars template: {}", err))
        })?;
    }

    Ok(Self(mailer))
  }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ImportNotionMailerParam {
  pub import_task_id: String,
  pub user_name: String,
  pub import_file_name: String,
  pub workspace_id: String,
  pub workspace_name: String,
  pub open_workspace: bool,
  pub error: Option<String>,
  pub error_detail: Option<String>,
}

#[cfg(test)]
mod tests {
  use crate::mailer::{AFWorkerMailer, ImportNotionMailerParam, IMPORT_SUCCESS_TEMPLATE};
  use mailer::sender::Mailer;

  #[tokio::test]
  async fn render_import_report() {
    let mailer = Mailer::new(
      "smtp_username".to_string(),
      "stmp_email".to_string(),
      "smtp_password".to_string().into(),
      "localhost",
      465,
    )
    .await
    .unwrap();
    let worker_mailer = AFWorkerMailer::new(mailer).await.unwrap();
    let value = serde_json::to_value(ImportNotionMailerParam {
      import_task_id: "test_task_id".to_string(),
      user_name: "nathan".to_string(),
      import_file_name: "working".to_string(),
      workspace_id: "1".to_string(),
      workspace_name: "working".to_string(),
      open_workspace: true,
      error: None,
      error_detail: None,
    })
    .unwrap();
    let s = worker_mailer
      .render(IMPORT_SUCCESS_TEMPLATE, &value)
      .unwrap();

    println!("{}", s);
  }
}
