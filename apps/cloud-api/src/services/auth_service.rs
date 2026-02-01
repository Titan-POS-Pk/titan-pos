//! Authentication gRPC service implementation.
//!
//! Handles API key exchange for JWT tokens.

use std::sync::Arc;

use tonic::{Request, Response, Status};
use tracing::{info, warn};

use crate::auth::JwtManager;
use crate::proto::{
    auth_service_server::AuthService,
    ExchangeTokenRequest, ExchangeTokenResponse,
    RefreshTokenRequest, RefreshTokenResponse,
    RevokeTokenRequest, RevokeTokenResponse,
};
use crate::AppState;

/// Authentication service implementation.
pub struct AuthServiceImpl {
    state: Arc<AppState>,
    jwt_manager: JwtManager,
}

impl AuthServiceImpl {
    /// Create a new authentication service.
    pub fn new(state: Arc<AppState>) -> Self {
        let jwt_manager = JwtManager::new(
            state.config.jwt_secret.clone(),
            state.config.jwt_access_lifetime_secs,
            state.config.jwt_refresh_lifetime_secs,
        );
        
        AuthServiceImpl { state, jwt_manager }
    }
}

#[tonic::async_trait]
impl AuthService for AuthServiceImpl {
    /// Exchange API key for JWT access token.
    async fn exchange_token(
        &self,
        request: Request<ExchangeTokenRequest>,
    ) -> Result<Response<ExchangeTokenResponse>, Status> {
        let req = request.into_inner();
        
        info!(
            store_id = %req.store_id,
            tenant_id = %req.tenant_id,
            device_id = %req.device_id,
            "Token exchange request"
        );

        // Validate the API key
        let store = self.state.db
            .validate_api_key(&req.api_key, &req.store_id, &req.tenant_id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        let store = match store {
            Some(s) => s,
            None => {
                warn!(
                    store_id = %req.store_id,
                    "Invalid API key or store not found"
                );
                return Err(Status::unauthenticated("Invalid API key or store"));
            }
        };

        // Generate tokens
        let access_token = self.jwt_manager
            .generate_access_token(&store.id, &store.tenant_id, &req.device_id)
            .map_err(|e| Status::internal(e.to_string()))?;

        let refresh_token = self.jwt_manager
            .generate_refresh_token(&store.id, &store.tenant_id, &req.device_id)
            .map_err(|e| Status::internal(e.to_string()))?;

        info!(
            store_id = %store.id,
            device_id = %req.device_id,
            "Token issued successfully"
        );

        Ok(Response::new(ExchangeTokenResponse {
            access_token,
            refresh_token,
            expires_in: self.state.config.jwt_access_lifetime_secs,
            token_type: "Bearer".to_string(),
        }))
    }

    /// Refresh an expiring token.
    async fn refresh_token(
        &self,
        request: Request<RefreshTokenRequest>,
    ) -> Result<Response<RefreshTokenResponse>, Status> {
        let req = request.into_inner();

        // Validate the refresh token
        let claims = self.jwt_manager
            .validate_refresh_token(&req.refresh_token)
            .map_err(|e| Status::unauthenticated(e.to_string()))?;

        // Generate new tokens
        let access_token = self.jwt_manager
            .generate_access_token(&claims.sub, &claims.tenant_id, &claims.device_id)
            .map_err(|e| Status::internal(e.to_string()))?;

        let refresh_token = self.jwt_manager
            .generate_refresh_token(&claims.sub, &claims.tenant_id, &claims.device_id)
            .map_err(|e| Status::internal(e.to_string()))?;

        info!(
            store_id = %claims.sub,
            device_id = %claims.device_id,
            "Token refreshed successfully"
        );

        Ok(Response::new(RefreshTokenResponse {
            access_token,
            refresh_token,
            expires_in: self.state.config.jwt_access_lifetime_secs,
        }))
    }

    /// Revoke a token (logout).
    async fn revoke_token(
        &self,
        request: Request<RevokeTokenRequest>,
    ) -> Result<Response<RevokeTokenResponse>, Status> {
        let req = request.into_inner();

        // In a full implementation, we would add the token to a blacklist
        // For now, we just acknowledge the request
        info!("Token revocation requested");

        // Validate the token exists and is valid
        let _ = self.jwt_manager
            .validate_token(&req.token)
            .map_err(|e| Status::invalid_argument(e.to_string()))?;

        // TODO: Add to token blacklist (Redis or database)

        Ok(Response::new(RevokeTokenResponse { success: true }))
    }
}
