//! Config gRPC service implementation.
//!
//! Provides store configuration management.

use std::sync::Arc;

use tonic::{Request, Response, Status};
use tracing::info;

use crate::auth::{extract_bearer_token, JwtManager};
use crate::proto::{
    config_service_server::ConfigService,
    GetConfigValueRequest, GetConfigValueResponse,
    GetStoreConfigRequest, GetStoreConfigResponse,
    StoreConfig as ProtoStoreConfig,
    UpdateConfigValueRequest, UpdateConfigValueResponse,
    Timestamp as ProtoTimestamp,
};
use crate::AppState;

/// Config service implementation.
pub struct ConfigServiceImpl {
    state: Arc<AppState>,
    jwt_manager: JwtManager,
}

impl ConfigServiceImpl {
    /// Create a new config service.
    pub fn new(state: Arc<AppState>) -> Self {
        let jwt_manager = JwtManager::new(
            state.config.jwt_secret.clone(),
            state.config.jwt_access_lifetime_secs,
            state.config.jwt_refresh_lifetime_secs,
        );
        
        ConfigServiceImpl { state, jwt_manager }
    }

    /// Authenticate a request from metadata.
    fn authenticate(&self, request: &Request<impl std::any::Any>) -> Result<(String, String), Status> {
        let auth_header = request
            .metadata()
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| Status::unauthenticated("Missing authorization header"))?;

        let token = extract_bearer_token(auth_header)
            .ok_or_else(|| Status::unauthenticated("Invalid authorization header"))?;

        let claims = self.jwt_manager
            .validate_access_token(token)
            .map_err(|e| Status::unauthenticated(e.to_string()))?;

        Ok((claims.sub, claims.tenant_id))
    }
}

#[tonic::async_trait]
impl ConfigService for ConfigServiceImpl {
    /// Get full store configuration.
    async fn get_store_config(
        &self,
        request: Request<GetStoreConfigRequest>,
    ) -> Result<Response<GetStoreConfigResponse>, Status> {
        let (store_id, _tenant_id) = self.authenticate(&request)?;
        let req = request.into_inner();

        // Verify the requested store matches the authenticated store
        if req.store_id != store_id {
            return Err(Status::permission_denied("Cannot access other store's configuration"));
        }

        info!(store_id = %store_id, "Fetching store configuration");

        let config = self.state.db
            .get_store_config(&store_id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        let config = match config {
            Some(c) => c,
            None => {
                return Err(Status::not_found("Store configuration not found"));
            }
        };

        let proto_config = ProtoStoreConfig {
            store_id: config.store_id,
            store_name: config.store_name,
            tenant_id: config.tenant_id,
            address: config.address.unwrap_or_default(),
            city: config.city.unwrap_or_default(),
            state: config.state.unwrap_or_default(),
            postal_code: config.postal_code.unwrap_or_default(),
            country: config.country.unwrap_or_default(),
            timezone: config.timezone.unwrap_or_else(|| "UTC".to_string()),
            currency: config.currency,
            tax_mode: config.tax_mode,
            allow_negative_inventory: config.allow_negative_inventory,
            receipt_header: config.receipt_header.unwrap_or_default(),
            receipt_footer: config.receipt_footer.unwrap_or_default(),
            sync_batch_size: config.sync_batch_size,
            sync_interval_secs: config.sync_interval_secs,
        };

        Ok(Response::new(GetStoreConfigResponse {
            config: Some(proto_config),
        }))
    }

    /// Get specific config value.
    async fn get_config_value(
        &self,
        request: Request<GetConfigValueRequest>,
    ) -> Result<Response<GetConfigValueResponse>, Status> {
        let (store_id, _tenant_id) = self.authenticate(&request)?;
        let req = request.into_inner();

        // Verify the requested store matches the authenticated store
        if req.store_id != store_id {
            return Err(Status::permission_denied("Cannot access other store's configuration"));
        }

        info!(store_id = %store_id, key = %req.key, "Fetching config value");

        // Get the full config and extract the requested key
        let config = self.state.db
            .get_store_config(&store_id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        let config = match config {
            Some(c) => c,
            None => {
                return Err(Status::not_found("Store configuration not found"));
            }
        };

        // Map key to config field
        let value = match req.key.as_str() {
            "store_name" => config.store_name,
            "currency" => config.currency,
            "tax_mode" => config.tax_mode,
            "timezone" => config.timezone.unwrap_or_else(|| "UTC".to_string()),
            "allow_negative_inventory" => config.allow_negative_inventory.to_string(),
            "sync_batch_size" => config.sync_batch_size.to_string(),
            "sync_interval_secs" => config.sync_interval_secs.to_string(),
            _ => {
                return Err(Status::not_found(format!("Config key not found: {}", req.key)));
            }
        };

        Ok(Response::new(GetConfigValueResponse {
            key: req.key,
            value,
            updated_at: None, // TODO: Track per-key update times
        }))
    }

    /// Update config value (if permitted).
    async fn update_config_value(
        &self,
        request: Request<UpdateConfigValueRequest>,
    ) -> Result<Response<UpdateConfigValueResponse>, Status> {
        let (store_id, _tenant_id) = self.authenticate(&request)?;
        let req = request.into_inner();

        // Verify the requested store matches the authenticated store
        if req.store_id != store_id {
            return Err(Status::permission_denied("Cannot modify other store's configuration"));
        }

        info!(store_id = %store_id, key = %req.key, "Updating config value");

        // For now, config updates from stores are not allowed
        // This would be implemented when we have admin functionality
        Err(Status::permission_denied("Store config updates are managed by tenant administrators"))
    }
}
