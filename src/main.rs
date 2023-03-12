use appflowy_server::application::{init_state, Application};
use appflowy_server::config::config::get_configuration;
use appflowy_server::telemetry::{get_subscriber, init_subscriber};

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let subscriber = get_subscriber("appflowy_server".into(), "info".into(), std::io::stdout);
    init_subscriber(subscriber);

    let configuration = get_configuration().expect("Failed to read configuration.");
    let state = init_state(&configuration).await;
    let application = Application::build(configuration, state).await?;
    application.run_until_stopped().await?;

    Ok(())
}
