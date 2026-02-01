//! # Titan Cloud API
//!
//! gRPC server for cloud synchronization with Store Hubs.
//!
//! ## Architecture
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                        Cloud API Server                                 │
//! │                                                                         │
//! │  Store Hub ───► gRPC (50051) ───► Services ───► PostgreSQL            │
//! │                                       │                                 │
//! │                                       ▼                                 │
//! │                                     Redis                               │
//! │                                  (Pub/Sub)                              │
//! └─────────────────────────────────────────────────────────────────────────┘
//! ```

mod proto;
mod config;
mod db;
mod error;
mod services;
mod auth;

use std::net::SocketAddr;
use std::sync::Arc;

use tonic::transport::Server;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

use crate::config::CloudConfig;
use crate::db::Database;
use crate::services::{
    auth_service::AuthServiceImpl,
    sync_service::SyncServiceImpl,
    config_service::ConfigServiceImpl,
    notification_service::NotificationServiceImpl,
    health_service::HealthServiceImpl,
};
use crate::proto::{
    auth_service_server::AuthServiceServer,
    sync_service_server::SyncServiceServer,
    config_service_server::ConfigServiceServer,
    notification_service_server::NotificationServiceServer,
    health_service_server::HealthServiceServer,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .with_target(true)
        .with_thread_ids(true)
        .pretty()
        .init();

    info!("Starting Titan Cloud API server...");

    // Load configuration
    let config = CloudConfig::load()?;
    info!(
        port = config.grpc_port,
        db_url = %config.database_url.chars().take(30).collect::<String>(),
        "Configuration loaded"
    );

    // Connect to database
    let db = Database::connect(&config.database_url).await?;
    info!("Connected to PostgreSQL");

    // Run migrations
    db.run_migrations().await?;
    info!("Database migrations complete");

    // Connect to Redis (optional for now)
    let redis = if let Some(ref redis_url) = config.redis_url {
        match redis::Client::open(redis_url.as_str()) {
            Ok(client) => {
                info!("Connected to Redis");
                Some(client)
            }
            Err(e) => {
                tracing::warn!(?e, "Failed to connect to Redis, continuing without it");
                None
            }
        }
    } else {
        None
    };

    // Create shared state
    let state = Arc::new(AppState {
        db,
        redis,
        config: config.clone(),
    });

    // Build gRPC services
    let auth_service = AuthServiceServer::new(AuthServiceImpl::new(state.clone()));
    let sync_service = SyncServiceServer::new(SyncServiceImpl::new(state.clone()));
    let config_service = ConfigServiceServer::new(ConfigServiceImpl::new(state.clone()));
    let notification_service = NotificationServiceServer::new(NotificationServiceImpl::new(state.clone()));
    let health_service = HealthServiceServer::new(HealthServiceImpl::new(state.clone()));

    // Build server address
    let addr: SocketAddr = format!("0.0.0.0:{}", config.grpc_port).parse()?;
    info!(%addr, "Starting gRPC server");

    // Start server
    Server::builder()
        .add_service(auth_service)
        .add_service(sync_service)
        .add_service(config_service)
        .add_service(notification_service)
        .add_service(health_service)
        .serve_with_shutdown(addr, shutdown_signal())
        .await?;

    info!("Server shutdown complete");
    Ok(())
}

/// Shared application state.
pub struct AppState {
    pub db: Database,
    pub redis: Option<redis::Client>,
    pub config: CloudConfig,
}

/// Graceful shutdown signal handler.
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("Failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    info!("Shutdown signal received, starting graceful shutdown...");
}
