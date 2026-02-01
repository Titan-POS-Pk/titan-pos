//! Cloud API configuration module.
//!
//! Configuration is loaded from environment variables with fallback to defaults.

use serde::{Deserialize, Serialize};
use std::env;

/// Cloud API configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudConfig {
    /// gRPC server port
    pub grpc_port: u16,

    /// PostgreSQL connection string
    pub database_url: String,

    /// Redis connection string (optional)
    pub redis_url: Option<String>,

    /// JWT secret key for signing tokens
    pub jwt_secret: String,

    /// JWT access token lifetime in seconds
    pub jwt_access_lifetime_secs: i64,

    /// JWT refresh token lifetime in seconds
    pub jwt_refresh_lifetime_secs: i64,

    /// Enable TLS for gRPC
    pub tls_enabled: bool,

    /// TLS certificate path
    pub tls_cert_path: Option<String>,

    /// TLS key path
    pub tls_key_path: Option<String>,

    /// Max message size in bytes (default: 16MB)
    pub max_message_size: usize,

    /// Sync batch size limit
    pub sync_batch_size_limit: usize,
}

impl CloudConfig {
    /// Load configuration from environment variables.
    pub fn load() -> Result<Self, ConfigError> {
        let config = CloudConfig {
            grpc_port: env::var("GRPC_PORT")
                .unwrap_or_else(|_| "50051".to_string())
                .parse()
                .map_err(|_| ConfigError::InvalidValue("GRPC_PORT".to_string()))?,

            database_url: env::var("DATABASE_URL")
                .unwrap_or_else(|_| {
                    "postgres://titan:titan_dev_password@localhost:5432/titan_pos".to_string()
                }),

            redis_url: env::var("REDIS_URL").ok(),

            jwt_secret: env::var("JWT_SECRET")
                .unwrap_or_else(|_| {
                    // Generate a random secret for development
                    // In production, this MUST be set via environment variable
                    "titan-cloud-dev-secret-change-in-production".to_string()
                }),

            jwt_access_lifetime_secs: env::var("JWT_ACCESS_LIFETIME_SECS")
                .unwrap_or_else(|_| "3600".to_string()) // 1 hour
                .parse()
                .map_err(|_| ConfigError::InvalidValue("JWT_ACCESS_LIFETIME_SECS".to_string()))?,

            jwt_refresh_lifetime_secs: env::var("JWT_REFRESH_LIFETIME_SECS")
                .unwrap_or_else(|_| "604800".to_string()) // 7 days
                .parse()
                .map_err(|_| ConfigError::InvalidValue("JWT_REFRESH_LIFETIME_SECS".to_string()))?,

            tls_enabled: env::var("TLS_ENABLED")
                .unwrap_or_else(|_| "false".to_string())
                .parse()
                .unwrap_or(false),

            tls_cert_path: env::var("TLS_CERT_PATH").ok(),

            tls_key_path: env::var("TLS_KEY_PATH").ok(),

            max_message_size: env::var("MAX_MESSAGE_SIZE")
                .unwrap_or_else(|_| "16777216".to_string()) // 16MB
                .parse()
                .map_err(|_| ConfigError::InvalidValue("MAX_MESSAGE_SIZE".to_string()))?,

            sync_batch_size_limit: env::var("SYNC_BATCH_SIZE_LIMIT")
                .unwrap_or_else(|_| "1000".to_string())
                .parse()
                .map_err(|_| ConfigError::InvalidValue("SYNC_BATCH_SIZE_LIMIT".to_string()))?,
        };

        // Validate TLS configuration
        if config.tls_enabled {
            if config.tls_cert_path.is_none() || config.tls_key_path.is_none() {
                return Err(ConfigError::MissingTlsConfig);
            }
        }

        Ok(config)
    }
}

/// Configuration error types.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("Invalid value for {0}")]
    InvalidValue(String),

    #[error("TLS enabled but certificate or key path not provided")]
    MissingTlsConfig,

    #[error("Missing required configuration: {0}")]
    MissingRequired(String),
}
