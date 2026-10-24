//! Binary entrypoint for the supervictor endpoint server.

use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = supervictor_endpoint::config::Config::from_env()?;

    tracing_subscriber::fmt()
        .json()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&config.log_level)),
        )
        .init();

    tracing::info!(
        environment = %config.environment,
        store_backend = %config.store_backend,
        "starting supervictor endpoint"
    );

    let store = supervictor_endpoint::store::factory::create_store(&config).await?;
    supervictor_endpoint::watchdog::spawn(store.clone(), config.watchdog_interval_secs);
    let app = supervictor_endpoint::routes::router_with(
        store,
        supervictor_endpoint::routes::RouterOptions {
            require_registration: config.require_registration,
        },
    );

    let addr = format!("0.0.0.0:{}", config.port);
    match config.tls {
        Some(tls) => {
            #[cfg(feature = "tls")]
            {
                let paths = supervictor_endpoint::tls::TlsPaths {
                    cert: tls.cert,
                    key: tls.key,
                    client_ca: tls.client_ca,
                };
                let listener = supervictor_endpoint::tls::TlsListener::bind(&addr, &paths).await?;
                tracing::info!(addr = %addr, "listening (mTLS terminated in-process)");
                axum::serve(
                    listener,
                    app.into_make_service_with_connect_info::<supervictor_endpoint::tls::TlsPeer>(),
                )
                .with_graceful_shutdown(shutdown_signal())
                .await?;
            }
            #[cfg(not(feature = "tls"))]
            {
                let _ = tls;
                return Err(supervictor_endpoint::error::AppError::Config(
                    "TLS_CERT/TLS_KEY/TLS_CLIENT_CA set but this binary was built without the `tls` feature".into(),
                )
                .into());
            }
        }
        None => {
            let listener = tokio::net::TcpListener::bind(&addr).await?;
            tracing::info!(addr = %addr, "listening (plain HTTP; identity from proxy headers)");
            axum::serve(listener, app)
                .with_graceful_shutdown(shutdown_signal())
                .await?;
        }
    }
    tracing::info!("shutdown complete");

    Ok(())
}

/// Resolve when SIGTERM (ECS task stop, Lambda Web Adapter drain) or Ctrl-C
/// arrives, letting in-flight requests finish instead of dropping mid-write.
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl-C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => tracing::info!("received Ctrl-C, draining"),
        _ = terminate => tracing::info!("received SIGTERM, draining"),
    }
}
