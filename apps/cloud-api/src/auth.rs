//! JWT authentication module.
//!
//! Handles JWT token generation, validation, and refresh.

use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, TokenData, Validation};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::CloudError;

/// JWT claims structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    /// Subject (store_id)
    pub sub: String,
    
    /// Tenant ID
    pub tenant_id: String,
    
    /// Device ID that requested the token
    pub device_id: String,
    
    /// Issued at (Unix timestamp)
    pub iat: i64,
    
    /// Expiration (Unix timestamp)
    pub exp: i64,
    
    /// JWT ID (unique identifier for this token)
    pub jti: String,
    
    /// Token type ("access" or "refresh")
    pub token_type: String,
}

/// JWT token manager.
pub struct JwtManager {
    secret: String,
    access_lifetime_secs: i64,
    refresh_lifetime_secs: i64,
}

impl JwtManager {
    /// Create a new JWT manager.
    pub fn new(secret: String, access_lifetime_secs: i64, refresh_lifetime_secs: i64) -> Self {
        JwtManager {
            secret,
            access_lifetime_secs,
            refresh_lifetime_secs,
        }
    }

    /// Generate an access token.
    pub fn generate_access_token(
        &self,
        store_id: &str,
        tenant_id: &str,
        device_id: &str,
    ) -> Result<String, CloudError> {
        let now = Utc::now();
        let exp = now + Duration::seconds(self.access_lifetime_secs);

        let claims = Claims {
            sub: store_id.to_string(),
            tenant_id: tenant_id.to_string(),
            device_id: device_id.to_string(),
            iat: now.timestamp(),
            exp: exp.timestamp(),
            jti: Uuid::new_v4().to_string(),
            token_type: "access".to_string(),
        };

        encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(self.secret.as_bytes()),
        )
        .map_err(|e| CloudError::Internal(format!("Failed to generate token: {}", e)))
    }

    /// Generate a refresh token.
    pub fn generate_refresh_token(
        &self,
        store_id: &str,
        tenant_id: &str,
        device_id: &str,
    ) -> Result<String, CloudError> {
        let now = Utc::now();
        let exp = now + Duration::seconds(self.refresh_lifetime_secs);

        let claims = Claims {
            sub: store_id.to_string(),
            tenant_id: tenant_id.to_string(),
            device_id: device_id.to_string(),
            iat: now.timestamp(),
            exp: exp.timestamp(),
            jti: Uuid::new_v4().to_string(),
            token_type: "refresh".to_string(),
        };

        encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(self.secret.as_bytes()),
        )
        .map_err(|e| CloudError::Internal(format!("Failed to generate refresh token: {}", e)))
    }

    /// Validate and decode a token.
    pub fn validate_token(&self, token: &str) -> Result<Claims, CloudError> {
        let validation = Validation::default();
        
        let token_data: TokenData<Claims> = decode(
            token,
            &DecodingKey::from_secret(self.secret.as_bytes()),
            &validation,
        )
        .map_err(|e| CloudError::AuthFailed(format!("Invalid token: {}", e)))?;

        Ok(token_data.claims)
    }

    /// Validate that a token is an access token.
    pub fn validate_access_token(&self, token: &str) -> Result<Claims, CloudError> {
        let claims = self.validate_token(token)?;
        
        if claims.token_type != "access" {
            return Err(CloudError::AuthFailed("Expected access token".to_string()));
        }

        Ok(claims)
    }

    /// Validate that a token is a refresh token.
    pub fn validate_refresh_token(&self, token: &str) -> Result<Claims, CloudError> {
        let claims = self.validate_token(token)?;
        
        if claims.token_type != "refresh" {
            return Err(CloudError::AuthFailed("Expected refresh token".to_string()));
        }

        Ok(claims)
    }

    /// Get remaining lifetime of a token in seconds.
    pub fn get_token_lifetime(&self, token: &str) -> Result<i64, CloudError> {
        let claims = self.validate_token(token)?;
        let now = Utc::now().timestamp();
        Ok(claims.exp - now)
    }
}

/// Extract bearer token from authorization header.
pub fn extract_bearer_token(auth_header: &str) -> Option<&str> {
    if auth_header.starts_with("Bearer ") {
        Some(&auth_header[7..])
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jwt_roundtrip() {
        let manager = JwtManager::new("test-secret".to_string(), 3600, 86400);
        
        let access_token = manager
            .generate_access_token("store-001", "tenant-001", "device-001")
            .unwrap();
        
        let claims = manager.validate_access_token(&access_token).unwrap();
        
        assert_eq!(claims.sub, "store-001");
        assert_eq!(claims.tenant_id, "tenant-001");
        assert_eq!(claims.device_id, "device-001");
        assert_eq!(claims.token_type, "access");
    }

    #[test]
    fn test_refresh_token() {
        let manager = JwtManager::new("test-secret".to_string(), 3600, 86400);
        
        let refresh_token = manager
            .generate_refresh_token("store-001", "tenant-001", "device-001")
            .unwrap();
        
        let claims = manager.validate_refresh_token(&refresh_token).unwrap();
        assert_eq!(claims.token_type, "refresh");
    }

    #[test]
    fn test_wrong_token_type() {
        let manager = JwtManager::new("test-secret".to_string(), 3600, 86400);
        
        let access_token = manager
            .generate_access_token("store-001", "tenant-001", "device-001")
            .unwrap();
        
        // Try to validate access token as refresh token
        let result = manager.validate_refresh_token(&access_token);
        assert!(result.is_err());
    }
}
