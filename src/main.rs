use appflowy_server::application::{init_state, Application};
use appflowy_server::component::log::init_log;
use appflowy_server::config::config::get_configuration;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    init_log();
    let configuration = get_configuration().expect("Failed to read configuration.");
    let state = init_state(&configuration).await;
    let application = Application::build(configuration, state).await?;
    application.run_until_stopped().await?;

    Ok(())
}
