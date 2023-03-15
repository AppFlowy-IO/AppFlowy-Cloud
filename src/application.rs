use crate::api::{token_scope, user_scope, ws_scope};
use crate::component::auth::HEADER_TOKEN;
use crate::config::config::{Config, DatabaseSetting};
use crate::middleware::cors::default_cors;
use crate::self_signed::create_certificate;
use crate::state::State;
use actix_identity::IdentityMiddleware;
use actix_session::storage::RedisSessionStore;
use actix_session::SessionMiddleware;
use actix_web::cookie::Key;
use actix_web::{dev::Server, web, web::Data, App, HttpServer};

use openssl::ssl::{SslAcceptor, SslAcceptorBuilder, SslFiletype, SslMethod};
use openssl::x509::X509;
use secrecy::{ExposeSecret, Secret};
use sqlx::{postgres::PgPoolOptions, PgPool};
use std::net::TcpListener;
use std::sync::Arc;
use tracing_actix_web::TracingLogger;

pub struct Application {
    port: u16,
    server: Server,
}

impl Application {
    pub async fn build(config: Config, state: State) -> Result<Self, anyhow::Error> {
        let address = format!("{}:{}", config.application.host, config.application.port);
        let listener = TcpListener::bind(&address)?;
        let port = listener.local_addr().unwrap().port();
        let server = run(
            listener,
            state,
            certificate,
            server_key,
            // config.application.server_key.clone(),
            config.redis_uri.clone(),
        )
        .await?;

        Ok(Self { port, server })
    }

    pub async fn run_until_stopped(self) -> Result<(), std::io::Error> {
        self.server.await
    }

    pub fn port(&self) -> u16 {
        self.port
    }
}

pub async fn run(
    listener: TcpListener,
    state: State,
    certificate: Secret<String>,
    secret_key: Secret<String>,
    redis_uri: Secret<String>,
) -> Result<Server, anyhow::Error> {
    let redis_store = RedisSessionStore::new(redis_uri.expose_secret()).await?;
    let server = HttpServer::new(move || {
        let secret_key = Key::from(secret_key.expose_secret().as_bytes());
        App::new()
            // Session middleware
            .wrap(
                SessionMiddleware::builder(redis_store.clone(), secret_key.clone())
                    .cookie_name(HEADER_TOKEN.to_string())
                    .build(),
            )
            .wrap(IdentityMiddleware::default())
            .wrap(default_cors())
            .wrap(TracingLogger::default())
            .app_data(web::JsonConfig::default().limit(4096))
            .service(user_scope())
            .service(token_scope())
            .service(ws_scope())
            .app_data(Data::new(state.clone()))
    })
    .listen_openssl(listener, make_ssl_acceptor_builder(cert))?
    .run();
    Ok(server)
}

pub async fn init_state(configuration: &Config) -> State {
    let pg_pool = get_connection_pool(&configuration.database)
        .await
        .unwrap_or_else(|_| {
            panic!(
                "Failed to connect to Postgres at {:?}.",
                configuration.database
            )
        });

    State {
        pg_pool,
        user: Arc::new(Default::default()),
    }
}

pub async fn get_connection_pool(setting: &DatabaseSetting) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .acquire_timeout(std::time::Duration::from_secs(5))
        .connect_with(setting.with_db())
        .await
}

fn make_ssl_acceptor_builder(cert: String) -> SslAcceptorBuilder {
    let mut builder = SslAcceptor::mozilla_intermediate(SslMethod::tls()).unwrap();
    let x509_cert = X509::from_pem(cert.as_bytes()).unwrap();
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
