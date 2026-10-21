mod config;
mod db;
mod error;
mod models;
mod routes;
mod seed;

use tokio::net::TcpListener;

pub use config::{database_url_from_env, Config};
pub use error::StartupError;
pub use seed::{resolve_content_dir, seed_from_content, SeedError, SeedReport};

pub use db::prepare_database;
use error::StartupError as BootError;
use routes::build_router;
use sqlx::SqlitePool;

#[derive(Clone)]
pub(crate) struct AppState {
    pool: SqlitePool,
    author_token: String,
}

pub async fn build_app(config: &Config) -> Result<axum::Router, StartupError> {
    let pool = prepare_database(&config.database_url).await?;
    Ok(build_router(AppState {
        pool,
        author_token: config.author_token.clone(),
    }))
}

pub async fn serve(config: Config) -> Result<(), StartupError> {
    let app = build_app(&config).await?;
    let listener = TcpListener::bind(&config.bind)
        .await
        .map_err(|error| BootError::new(format!("bind {}: {error}", config.bind)))?;
    let address = listener
        .local_addr()
        .map_err(|error| BootError::new(error.to_string()))?;
    tracing::info!(%address, "DevChronicle API listening");

    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            if tokio::signal::ctrl_c().await.is_err() {
                tracing::warn!("shutdown signal is not available");
            }
            tracing::info!("shutdown signal received");
        })
        .await
        .map_err(|error| BootError::new(error.to_string()))?;
    Ok(())
}
