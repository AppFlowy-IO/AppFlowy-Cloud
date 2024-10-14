use handlebars::Handlebars;
use lettre::message::header::ContentType;
use lettre::message::Message;
use lettre::transport::smtp::authentication::Credentials;
use lettre::Address;
use lettre::AsyncSmtpTransport;
use lettre::AsyncTransport;

#[derive(Clone)]
pub struct Mailer {
  smtp_transport: AsyncSmtpTransport<lettre::Tokio1Executor>,
  smtp_username: String,
  handlers: Handlebars<'static>,
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
    let handlers = Handlebars::new();
    Ok(Self {
      smtp_transport,
      smtp_username,
      handlers,
    })
  }

  pub async fn register_template(
    &mut self,
    name: &str,
    template: &str,
  ) -> Result<(), anyhow::Error> {
    self.handlers.register_template_string(name, template)?;
    Ok(())
  }

  pub fn render<T>(&self, name: &str, param: &T) -> Result<String, anyhow::Error>
  where
    T: serde::Serialize,
  {
    let rendered = self.handlers.render(name, param)?;
    Ok(rendered)
  }

  pub async fn send_email_template<T>(
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
    let rendered = self.handlers.render(template_name, &param)?;
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
}
