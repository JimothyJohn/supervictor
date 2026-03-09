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
    let app = supervictor_endpoint::routes::router(store);

    let addr = format!("0.0.0.0:{}", config.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!(addr = %addr, "listening");
    axum::serve(listener, app).await?;

    Ok(())
}
