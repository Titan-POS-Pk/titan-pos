//! # Cloud Authentication Manager
//!
//! This module handles JWT token management for cloud API authentication.
//! It exchanges the store's API key for JWT tokens and handles automatic
//! token refresh before expiration.
//!
//! ## Authentication Flow
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                      Cloud Authentication Flow                          │
//! │                                                                         │
//! │  ┌────────────────┐     ┌─────────────────┐     ┌─────────────────┐    │
//! │  │  titan-sync    │     │  Cloud API      │     │  PostgreSQL     │    │
//! │  │  (PRIMARY)     │     │  (gRPC)         │     │  (stores table) │    │
//! │  └───────┬────────┘     └────────┬────────┘     └────────┬────────┘    │
//! │          │                       │                       │             │
//! │          │  1. ExchangeToken     │                       │             │
//! │          │    (api_key, device)  │                       │             │
//! │          │──────────────────────►│                       │             │
//! │          │                       │  2. Lookup store      │             │
//! │          │                       │     by api_key_hash   │             │
//! │          │                       │──────────────────────►│             │
//! │          │                       │◄──────────────────────│             │
//! │          │                       │  store_id, tenant_id  │             │
//! │          │  3. JWT + refresh     │                       │             │
//! │          │◄──────────────────────│                       │             │
//! │          │                       │                       │             │
//! │          │  [Later: Token near expiry]                   │             │
//! │          │                       │                       │             │
//! │          │  4. RefreshToken      │                       │             │
//! │          │    (refresh_token)    │                       │             │
//! │          │──────────────────────►│                       │             │
//! │          │  5. New JWT           │                       │             │
//! │          │◄──────────────────────│                       │             │
//! │          │                       │                       │             │
//! └─────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Token Storage
//! Tokens are stored in memory with automatic refresh scheduling.
//! The refresh happens 5 minutes before expiration to ensure seamless operation.

use crate::error::{SyncError, SyncResult};
use crate::proto::{auth_service_client::AuthServiceClient, ExchangeTokenRequest, RefreshTokenRequest, RevokeTokenRequest};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tonic::transport::{Channel, Endpoint};
use tonic::metadata::MetadataValue;
use tracing::{debug, error, info, warn};

/// Margin before token expiration to trigger refresh (5 minutes)
const REFRESH_MARGIN_SECS: u64 = 300;

/// Token information stored after authentication
#[derive(Debug, Clone)]
pub struct TokenInfo {
    /// The JWT access token
    pub access_token: String,
    /// When the access token expires (local time)
    pub expires_at: Instant,
    /// Refresh token for getting new access tokens
    pub refresh_token: String,
    /// Store ID from the cloud
    pub store_id: String,
    /// Tenant ID from the cloud
    pub tenant_id: String,
}

impl TokenInfo {
    /// Check if the token is expired or about to expire
    pub fn needs_refresh(&self) -> bool {
        let now = Instant::now();
        let margin = Duration::from_secs(REFRESH_MARGIN_SECS);
        now + margin >= self.expires_at
    }
    
    /// Check if the token is completely expired (no grace period)
    pub fn is_expired(&self) -> bool {
        Instant::now() >= self.expires_at
    }
    
    /// Get remaining valid time
    pub fn remaining_secs(&self) -> u64 {
        let now = Instant::now();
        if now >= self.expires_at {
            0
        } else {
            (self.expires_at - now).as_secs()
        }
    }
}

/// Configuration for cloud authentication
#[derive(Debug, Clone)]
pub struct CloudAuthConfig {
    /// Cloud API endpoint URL (e.g., "https://api.titanpos.io:50051")
    pub cloud_url: String,
    /// Store ID for this store
    pub store_id: String,
    /// Tenant ID for this store
    pub tenant_id: String,
    /// Store's API key for initial authentication
    pub api_key: String,
    /// Device ID for this POS terminal
    pub device_id: String,
    /// Device name (optional, for logging in cloud)
    pub device_name: Option<String>,
    /// Enable TLS verification (should be true in production)
    pub verify_tls: bool,
}

impl CloudAuthConfig {
    /// Create a new config from environment variables or provided values
    pub fn from_env_or(
        cloud_url: Option<String>,
        store_id: String,
        tenant_id: String,
        api_key: Option<String>,
        device_id: String,
        device_name: Option<String>,
    ) -> Self {
        Self {
            cloud_url: cloud_url
                .or_else(|| std::env::var("TITAN_CLOUD_URL").ok())
                .unwrap_or_else(|| "http://localhost:50051".to_string()),
            store_id,
            tenant_id,
            api_key: api_key
                .or_else(|| std::env::var("TITAN_API_KEY").ok())
                .unwrap_or_default(),
            device_id,
            device_name,
            verify_tls: std::env::var("TITAN_VERIFY_TLS")
                .map(|v| v != "false" && v != "0")
                .unwrap_or(true),
        }
    }
}

/// Cloud authentication manager
///
/// Handles API key exchange for JWT and automatic token refresh.
pub struct CloudAuth {
    /// Configuration
    config: CloudAuthConfig,
    /// Current token (if authenticated)
    token: Arc<RwLock<Option<TokenInfo>>>,
    /// gRPC channel (lazily initialized)
    channel: Arc<RwLock<Option<Channel>>>,
}

impl CloudAuth {
    /// Create a new cloud auth manager
    pub fn new(config: CloudAuthConfig) -> SyncResult<Self> {
        Ok(Self {
            config,
            token: Arc::new(RwLock::new(None)),
            channel: Arc::new(RwLock::new(None)),
        })
    }
    
    /// Perform initial authentication
    pub async fn authenticate(&self) -> SyncResult<()> {
        let token_info = self.do_authenticate().await?;
        let mut guard = self.token.write().await;
        *guard = Some(token_info);
        info!("Authenticated successfully");
        Ok(())
    }
    
    /// Get the current access token (alias for get_token)
    pub async fn get_access_token(&self) -> SyncResult<String> {
        self.get_token().await
    }
    
    /// Get the current access token if valid, or authenticate/refresh as needed
    ///
    /// ## Flow
    /// 1. If no token, perform initial authentication
    /// 2. If token needs refresh, refresh it
    /// 3. Return the valid access token
    pub async fn get_token(&self) -> SyncResult<String> {
        // Check current token state
        {
            let token_guard = self.token.read().await;
            if let Some(token) = token_guard.as_ref() {
                if !token.needs_refresh() {
                    debug!(remaining_secs = token.remaining_secs(), "Using cached token");
                    return Ok(token.access_token.clone());
                }
            }
        }
        
        // Need to refresh or authenticate
        let mut token_guard = self.token.write().await;
        
        // Double-check after acquiring write lock
        if let Some(token) = token_guard.as_ref() {
            if !token.needs_refresh() {
                return Ok(token.access_token.clone());
            }
            
            // Try to refresh if we have a refresh token and token isn't fully expired
            if !token.is_expired() {
                match self.do_refresh(&token.refresh_token).await {
                    Ok(new_token) => {
                        info!(
                            store_id = %new_token.store_id,
                            expires_in_secs = new_token.remaining_secs(),
                            "Token refreshed successfully"
                        );
                        let access_token = new_token.access_token.clone();
                        *token_guard = Some(new_token);
                        return Ok(access_token);
                    }
                    Err(e) => {
                        warn!(?e, "Token refresh failed, will re-authenticate");
                    }
                }
            }
        }
        
        // Need fresh authentication
        let new_token = self.do_authenticate().await?;
        info!(
            store_id = %new_token.store_id,
            tenant_id = %new_token.tenant_id,
            expires_in_secs = new_token.remaining_secs(),
            "Authenticated with cloud"
        );
        let access_token = new_token.access_token.clone();
        *token_guard = Some(new_token);
        
        Ok(access_token)
    }
    
    /// Get current token info (without triggering refresh)
    pub async fn current_token(&self) -> Option<TokenInfo> {
        self.token.read().await.clone()
    }
    
    /// Check if we have a valid token
    pub async fn is_authenticated(&self) -> bool {
        if let Some(token) = self.token.read().await.as_ref() {
            !token.is_expired()
        } else {
            false
        }
    }
    
    /// Get the store ID from the current token
    pub async fn store_id(&self) -> Option<String> {
        self.token.read().await.as_ref().map(|t| t.store_id.clone())
    }
    
    /// Get the tenant ID from the current token
    pub async fn tenant_id(&self) -> Option<String> {
        self.token.read().await.as_ref().map(|t| t.tenant_id.clone())
    }
    
    /// Logout / revoke the current token
    pub async fn logout(&self) -> SyncResult<()> {
        let token = {
            let guard = self.token.read().await;
            guard.as_ref().map(|t| t.access_token.clone())
        };
        
        if let Some(access_token) = token {
            // Try to revoke on server
            if let Err(e) = self.do_revoke(&access_token).await {
                warn!(?e, "Failed to revoke token on server");
            }
        }
        
        // Clear local token
        *self.token.write().await = None;
        info!("Logged out from cloud");
        
        Ok(())
    }
    
    /// Get or create the gRPC channel
    async fn get_channel(&self) -> SyncResult<Channel> {
        // Check if we have a cached channel
        {
            let guard = self.channel.read().await;
            if let Some(channel) = guard.as_ref() {
                return Ok(channel.clone());
            }
        }
        
        // Create new channel
        let mut channel_guard = self.channel.write().await;
        
        // Double-check after acquiring write lock
        if let Some(channel) = channel_guard.as_ref() {
            return Ok(channel.clone());
        }
        
        debug!(url = %self.config.cloud_url, "Connecting to cloud API");
        
        let endpoint = Endpoint::from_shared(self.config.cloud_url.clone())
            .map_err(|e| SyncError::Connection(format!("Invalid cloud URL: {}", e)))?
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10));
        
        // TODO: Add TLS configuration based on verify_tls
        
        let channel = endpoint
            .connect()
            .await
            .map_err(|e| SyncError::Connection(format!("Failed to connect to cloud: {}", e)))?;
        
        *channel_guard = Some(channel.clone());
        
        Ok(channel)
    }
    
    /// Perform initial authentication with API key
    async fn do_authenticate(&self) -> SyncResult<TokenInfo> {
        let channel = self.get_channel().await?;
        let mut client = AuthServiceClient::new(channel);
        
        let request = tonic::Request::new(ExchangeTokenRequest {
            api_key: self.config.api_key.clone(),
            store_id: self.config.store_id.clone(),
            tenant_id: self.config.tenant_id.clone(),
            device_id: self.config.device_id.clone(),
            device_name: self.config.device_name.clone().unwrap_or_default(),
        });
        
        let response = client
            .exchange_token(request)
            .await
            .map_err(|e| SyncError::AuthFailed(format!("Token exchange failed: {}", e)))?;
        
        let resp = response.into_inner();
        
        // Calculate expiration time
        let expires_at = Instant::now() + Duration::from_secs(resp.expires_in as u64);
        
        Ok(TokenInfo {
            access_token: resp.access_token,
            expires_at,
            refresh_token: resp.refresh_token,
            store_id: self.config.store_id.clone(),
            tenant_id: self.config.tenant_id.clone(),
        })
    }
    
    /// Refresh an existing token
    async fn do_refresh(&self, refresh_token: &str) -> SyncResult<TokenInfo> {
        let channel = self.get_channel().await?;
        let mut client = AuthServiceClient::new(channel);
        
        let request = tonic::Request::new(RefreshTokenRequest {
            refresh_token: refresh_token.to_string(),
        });
        
        let response = client
            .refresh_token(request)
            .await
            .map_err(|e| SyncError::AuthFailed(format!("Token refresh failed: {}", e)))?;
        
        let resp = response.into_inner();
        let expires_at = Instant::now() + Duration::from_secs(resp.expires_in as u64);
        
        // Get current store/tenant IDs (refresh doesn't return them)
        let (store_id, tenant_id) = {
            let guard = self.token.read().await;
            guard.as_ref()
                .map(|t| (t.store_id.clone(), t.tenant_id.clone()))
                .unwrap_or_default()
        };
        
        Ok(TokenInfo {
            access_token: resp.access_token,
            expires_at,
            refresh_token: resp.refresh_token,
            store_id,
            tenant_id,
        })
    }
    
    /// Revoke a token on the server
    async fn do_revoke(&self, access_token: &str) -> SyncResult<()> {
        let channel = self.get_channel().await?;
        let mut client = AuthServiceClient::new(channel);
        
        // Add authorization header
        let mut request = tonic::Request::new(RevokeTokenRequest {
            token: access_token.to_string(),
        });
        let token_value = format!("Bearer {}", access_token)
            .parse::<MetadataValue<_>>()
            .map_err(|_| SyncError::AuthFailed("Invalid token format".to_string()))?;
        request.metadata_mut().insert("authorization", token_value);
        
        client
            .revoke_token(request)
            .await
            .map_err(|e| SyncError::AuthFailed(format!("Token revocation failed: {}", e)))?;
        
        Ok(())
    }
}

/// Interceptor for adding authorization headers to gRPC requests
///
/// Use this to create authenticated clients for other services:
/// ```rust,ignore
/// let auth = CloudAuth::new(config);
/// let token = auth.get_token().await?;
/// let interceptor = AuthInterceptor::new(token);
/// let client = SyncServiceClient::with_interceptor(channel, interceptor);
/// ```
#[derive(Clone)]
pub struct AuthInterceptor {
    token: String,
}

impl AuthInterceptor {
    pub fn new(token: String) -> Self {
        Self { token }
    }
}

impl tonic::service::Interceptor for AuthInterceptor {
    fn call(&mut self, mut request: tonic::Request<()>) -> Result<tonic::Request<()>, tonic::Status> {
        let token_value = format!("Bearer {}", self.token)
            .parse::<MetadataValue<_>>()
            .map_err(|_| tonic::Status::invalid_argument("Invalid token"))?;
        request.metadata_mut().insert("authorization", token_value);
        Ok(request)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_token_needs_refresh() {
        let token = TokenInfo {
            access_token: "test".to_string(),
            expires_at: Instant::now() + Duration::from_secs(60), // 1 minute
            refresh_token: "refresh".to_string(),
            store_id: "store1".to_string(),
            tenant_id: "tenant1".to_string(),
        };
        
        // With only 1 minute left and 5 minute margin, should need refresh
        assert!(token.needs_refresh());
        assert!(!token.is_expired());
    }
    
    #[test]
    fn test_token_no_refresh_needed() {
        let token = TokenInfo {
            access_token: "test".to_string(),
            expires_at: Instant::now() + Duration::from_secs(3600), // 1 hour
            refresh_token: "refresh".to_string(),
            store_id: "store1".to_string(),
            tenant_id: "tenant1".to_string(),
        };
        
        // With 1 hour left and 5 minute margin, should not need refresh
        assert!(!token.needs_refresh());
        assert!(!token.is_expired());
    }
    
    #[test]
    fn test_config_from_env() {
        let config = CloudAuthConfig::from_env_or(
            Some("http://cloud.example.com:50051".to_string()),
            "store-001".to_string(),
            "tenant-001".to_string(),
            Some("test-api-key".to_string()),
            "device-001".to_string(),
            Some("Register 1".to_string()),
        );
        
        assert_eq!(config.cloud_url, "http://cloud.example.com:50051");
        assert_eq!(config.store_id, "store-001");
        assert_eq!(config.tenant_id, "tenant-001");
        assert_eq!(config.api_key, "test-api-key");
        assert_eq!(config.device_id, "device-001");
        assert_eq!(config.device_name, Some("Register 1".to_string()));
    }
}
