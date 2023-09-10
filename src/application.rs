use crate::api::{user_scope, ws_scope};
use crate::component::auth::HEADER_TOKEN;
use crate::config::config::{Config, DatabaseSetting, TlsConfig};
use crate::middleware::cors::default_cors;
use crate::self_signed::create_self_signed_certificate;
use crate::state::{State, Storage};
use actix_identity::IdentityMiddleware;
use actix_session::storage::RedisSessionStore;
use actix_session::SessionMiddleware;
use actix_web::cookie::Key;
use actix_web::{dev::Server, web, web::Data, App, HttpServer};

use actix::Actor;

use openssl::ssl::{SslAcceptor, SslAcceptorBuilder, SslFiletype, SslMethod};
use openssl::x509::X509;
use secrecy::{ExposeSecret, Secret};
use snowflake::Snowflake;
use sqlx::{postgres::PgPoolOptions, PgPool};
use std::net::TcpListener;
use std::sync::Arc;

use tokio::sync::RwLock;

use realtime::core::CollabManager;
use storage::collab::{CollabStorage, CollabStoragePGImpl};
use tracing_actix_web::TracingLogger;

pub struct Application {
  port: u16,
  server: Server,
}

impl Application {
  pub async fn build<S>(
    config: Config,
    state: State,
    storage: Storage<S>,
  ) -> Result<Self, anyhow::Error>
  where
    S: CollabStorage + Unpin,
  {
    let address = format!("{}:{}", config.application.host, config.application.port);
    let listener = TcpListener::bind(&address)?;
    let port = listener.local_addr().unwrap().port();
    let server = run(listener, state, config, storage).await?;

    Ok(Self { port, server })
  }

  pub async fn run_until_stopped(self) -> Result<(), std::io::Error> {
    self.server.await
  }

  pub fn port(&self) -> u16 {
    self.port
  }
}

pub async fn run<S>(
  listener: TcpListener,
  state: State,
  config: Config,
  storage: Storage<S>,
) -> Result<Server, anyhow::Error>
where
  S: CollabStorage + Unpin,
{
  let redis_store = RedisSessionStore::new(config.redis_uri.expose_secret())
    .await
    .map_err(|e| {
      anyhow::anyhow!(
        "Failed to connect to Redis at {:?}: {:?}",
        config.redis_uri,
        e
      )
    })?;
  let pair = get_certificate_and_server_key(&config);
  let key = pair
    .as_ref()
    .map(|(_, server_key)| Key::from(server_key.expose_secret().as_bytes()))
    .unwrap_or_else(Key::generate);

  let collab_server = CollabManager::new(storage.collab_storage.clone())
    .unwrap()
    .start();
  let mut server = HttpServer::new(move || {
    App::new()
      .wrap(
        SessionMiddleware::builder(redis_store.clone(), key.clone())
          .cookie_name(HEADER_TOKEN.to_string())
          .build(),
      )
      // .wrap(ErrorHandlers::new().handler(StatusCode::INTERNAL_SERVER_ERROR, add_error_header))
      .wrap(IdentityMiddleware::default())
      .wrap(default_cors())
      .wrap(TracingLogger::default())
      .app_data(web::JsonConfig::default().limit(4096))
      .service(user_scope())
      .service(ws_scope())
      .app_data(Data::new(collab_server.clone()))
      .app_data(Data::new(state.clone()))
      .app_data(Data::new(storage.clone()))
      .app_data(Data::new(gotrue::api::Client::new(
        reqwest::Client::new(),
        &config.gotrue.base_url)))
  });

  server = match pair {
    None => server.listen(listener)?,
    Some((certificate, _)) => {
      server.listen_openssl(listener, make_ssl_acceptor_builder(certificate))?
    },
  };

  Ok(server.run())
}

fn get_certificate_and_server_key(config: &Config) -> Option<(Secret<String>, Secret<String>)> {
  let tls_config = config.application.tls_config.as_ref()?;
  match tls_config {
    TlsConfig::NoTls => None,
    TlsConfig::SelfSigned => Some(create_self_signed_certificate().unwrap()),
  }
}

pub async fn init_state(config: &Config) -> State {
  let pg_pool = get_connection_pool(&config.database).await;

  State {
    pg_pool,
    config: Arc::new(config.clone()),
    user: Arc::new(Default::default()),
    id_gen: Arc::new(RwLock::new(Snowflake::new(1))),
  }
}

async fn get_connection_pool(setting: &DatabaseSetting) -> PgPool {
  PgPoolOptions::new()
    .acquire_timeout(std::time::Duration::from_secs(5))
    .connect_with(setting.with_db())
    .await
    .expect("Failed to connect to Postgres")
}

fn make_ssl_acceptor_builder(certificate: Secret<String>) -> SslAcceptorBuilder {
  let mut builder = SslAcceptor::mozilla_intermediate(SslMethod::tls()).unwrap();
  let x509_cert = X509::from_pem(certificate.expose_secret().as_bytes()).unwrap();
  builder.set_certificate(&x509_cert).unwrap();
  builder
    .set_private_key_file("./cert/key.pem", SslFiletype::PEM)
    .unwrap();
  builder
    .set_certificate_chain_file("./cert/cert.pem")
    .unwrap();
  builder
    .set_min_proto_version(Some(openssl::ssl::SslVersion::TLS1_2))
    .unwrap();
  builder
    .set_max_proto_version(Some(openssl::ssl::SslVersion::TLS1_3))
    .unwrap();
  builder
}

pub async fn init_storage(_config: &Config, pg_pool: PgPool) -> Storage<CollabStoragePGImpl> {
  let collab_storage = CollabStoragePGImpl::new(pg_pool);
  Storage { collab_storage }
}
