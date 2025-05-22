use handlebars::Handlebars;
use lettre::Address;
use lettre::AsyncSmtpTransport;
use lettre::AsyncTransport;
use lettre::message::Message;
use lettre::message::header::ContentType;
use lettre::transport::smtp::authentication::Credentials;
use lettre::transport::smtp::client::Tls;
use lettre::transport::smtp::client::TlsParameters;
use secrecy::ExposeSecret;

#[derive(Clone)]
pub struct Mailer {
  smtp_transport: AsyncSmtpTransport<lettre::Tokio1Executor>,
  smtp_email: String,
  handlers: Handlebars<'static>,
}
impl Mailer {
  pub async fn new(
    smtp_username: String,
    smtp_email: String,
    smtp_password: secrecy::Secret<String>,
    smtp_host: &str,
    smtp_port: u16,
    smtp_tls_kind: &str,
  ) -> Result<Self, anyhow::Error> {
    let creds = Credentials::new(smtp_username, smtp_password.expose_secret().to_string());
    let tls: Tls = match smtp_tls_kind {
      "none" => Tls::None,
      "wrapper" => Tls::Wrapper(TlsParameters::new(smtp_host.into())?),
      "required" => Tls::Required(TlsParameters::new(smtp_host.into())?),
      "opportunistic" => Tls::Opportunistic(TlsParameters::new(smtp_host.into())?),
      _ => return Err(anyhow::anyhow!("Invalid TLS kind")),
    };

    let smtp_transport = AsyncSmtpTransport::<lettre::Tokio1Executor>::builder_dangerous(smtp_host)
      .tls(tls)
      .credentials(creds)
      .port(smtp_port)
      .build();
    let handlers = Handlebars::new();
    Ok(Self {
      smtp_transport,
      smtp_email,
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
        self.smtp_email.parse::<Address>()?,
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
