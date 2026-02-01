//! # Titan Cloud API
//!
//! gRPC server for cloud synchronization with Store Hubs.
//!
//! ## Architecture
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                         Cloud API Services                              │
//! │                                                                         │
//! │  ┌────────────────┐  ┌────────────────┐  ┌────────────────────────────┐│
//! │  │  AuthService   │  │  SyncService   │  │  ConfigService             ││
//! │  │                │  │                │  │                            ││
//! │  │ • ExchangeToken│  │ • UploadBatch  │  │ • GetStoreConfig           ││
//! │  │ • RefreshToken │  │ • StreamUpload │  │ • GetConfigValue           ││
//! │  │ • RevokeToken  │  │ • GetPending   │  │ • UpdateConfigValue        ││
//! │  └────────────────┘  └────────────────┘  └────────────────────────────┘│
//! │                                                                         │
//! │  ┌────────────────┐  ┌────────────────┐                                │
//! │  │NotificationSvc │  │  HealthService │                                │
//! │  │                │  │                │                                │
//! │  │ • Subscribe    │  │ • Check        │                                │
//! │  │ (bidirectional)│  │ • Watch        │                                │
//! │  └────────────────┘  └────────────────┘                                │
//! │                                                                         │
//! │  ┌──────────────────────────────────────────────────────────────────┐  │
//! │  │                      Infrastructure                               │  │
//! │  │                                                                   │  │
//! │  │  ┌──────────────┐  ┌──────────────┐  ┌──────────────────────────┐│  │
//! │  │  │  PostgreSQL  │  │    Redis     │  │    JWT Auth              ││  │
//! │  │  │              │  │              │  │                          ││  │
//! │  │  │ Primary data │  │ Caching      │  │ Token management         ││  │
//! │  │  │ store        │  │ Pub/Sub      │  │ API key exchange         ││  │
//! │  │  └──────────────┘  └──────────────┘  └──────────────────────────┘│  │
//! │  └──────────────────────────────────────────────────────────────────┘  │
//! └─────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Configuration
//! Environment variables:
//! - `DATABASE_URL` - PostgreSQL connection string
//! - `REDIS_URL` - Redis connection string
//! - `GRPC_PORT` - gRPC server port (default: 50051)
//! - `JWT_SECRET` - Secret for JWT signing
//! - `JWT_ACCESS_EXPIRY_SECS` - Access token lifetime (default: 3600)
//! - `JWT_REFRESH_EXPIRY_SECS` - Refresh token lifetime (default: 604800)

pub mod auth;
pub mod config;
pub mod db;
pub mod error;
pub mod proto;
pub mod services;

// Re-exports
pub use config::CloudConfig;
pub use db::Database;
pub use error::CloudError;

/// Shared application state.
pub struct AppState {
    pub db: Database,
    pub redis: Option<redis::Client>,
    pub config: CloudConfig,
}
