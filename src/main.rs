use appflowy_cloud::application::{init_state, Application};
use appflowy_cloud::config::config::{get_configuration, Config};
use appflowy_cloud::telemetry::init_subscriber;
use pyroscope::pyroscope::PyroscopeAgentRunning;
use pyroscope::PyroscopeAgent;
use pyroscope_pprofrs::{pprof_backend, PprofConfig};
use tracing::info;

#[actix_web::main]
async fn main() -> anyhow::Result<()> {
  let level = std::env::var("RUST_LOG").unwrap_or("info".to_string());
  println!("AppFlowy Cloud with RUST_LOG={}", level);
  let mut filters = vec![];
  filters.push(format!("actix_web={}", level));
  filters.push(format!("collab={}", level));
  filters.push(format!("collab_sync={}", level));
  filters.push(format!("appflowy_cloud={}", level));
  filters.push(format!("collab_plugins={}", level));
  filters.push(format!("realtime={}", level));
  filters.push(format!("database={}", level));
  filters.push(format!("storage={}", level));
  filters.push(format!("gotrue={}", level));
  let conf =
    get_configuration().map_err(|e| anyhow::anyhow!("Failed to read configuration: {}", e))?;
  init_subscriber(&conf.app_env, filters);

  // If current build is debug and the feature "custom_env" is not enabled, load from .env
  // otherwise, load from .env.without_nginx.
  if cfg!(debug_assertions) {
    #[cfg(not(feature = "custom_env"))]
    {
      info!("custom_env is disable, load from .env");
      dotenvy::dotenv().ok();
    }

    #[cfg(feature = "custom_env")]
    {
      match dotenvy::from_filename(".env.without_nginx") {
        Ok(_) => {
          info!("custom_env is enabled, load from .env.without_nginx");
        },
        Err(err) => {
          tracing::error!(
            "Failed to load .env.without_nginx: {}, fallback to .env file",
            err
          );
          dotenvy::dotenv().ok();
        },
      }
    }
  } else {
    // In release, always load from .env
    dotenvy::dotenv().ok();
  }

  let _pyro_agent_running = init_pyroscope(&conf)?;

  let state = init_state(&conf)
    .await
    .map_err(|e| anyhow::anyhow!("Failed to initialize application state: {}", e))?;
  let application = Application::build(conf, state).await?;
  application.run_until_stopped().await?;

  Ok(())
}

// https://grafana.com/docs/pyroscope/latest/configure-client/language-sdks/rust/
fn init_pyroscope(conf: &Config) -> anyhow::Result<PyroscopeAgent<PyroscopeAgentRunning>> {
  let pyroscope_url = conf.pyroscope_url.as_str();
  info!("Pyroscope URL: {}", pyroscope_url);

  // Configure profiling backend
  let pprof_config = PprofConfig::new().sample_rate(100);
  let backend_impl = pprof_backend(pprof_config);

  // Configure Pyroscope Agent
  // let agent = PyroscopeAgent::builder(conf.pyroscope_url.as_str(), "appflowy-cloud")
  let agent = PyroscopeAgent::builder(pyroscope_url, "appflowy-cloud")
    .backend(backend_impl)
    .build()?;
  let pyro_agent_running = agent.start()?;
  Ok(pyro_agent_running)
}
