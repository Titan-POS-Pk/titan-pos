//! # Cloud Uplink - gRPC Client for Cloud Synchronization
//!
//! This module provides the gRPC client that PRIMARY nodes use to communicate
//! with the cloud API. It handles uploading sales/inventory data and receiving
//! product/configuration updates.
//!
//! ## Architecture
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                        Cloud Uplink Architecture                        │
//! │                                                                         │
//! │  ┌────────────────────────────────────────────────────────────────────┐│
//! │  │                    CloudUplink (this module)                       ││
//! │  │                                                                    ││
//! │  │  ┌──────────────┐  ┌──────────────┐  ┌──────────────────────────┐ ││
//! │  │  │ CloudAuth    │  │ SyncClient   │  │ NotificationSubscriber   │ ││
//! │  │  │              │  │              │  │                          │ ││
//! │  │  │ JWT tokens   │  │ UploadBatch  │  │ Bidirectional stream     │ ││
//! │  │  │ Auto-refresh │  │ StreamUpload │  │ Real-time updates        │ ││
//! │  │  │              │  │ GetPending   │  │ Heartbeat handling       │ ││
//! │  │  └──────────────┘  └──────────────┘  └──────────────────────────┘ ││
//! │  └────────────────────────────────────────────────────────────────────┘│
//! │                                 │                                       │
//! │                                 │ gRPC over HTTP/2                     │
//! │                                 ▼                                       │
//! │  ┌────────────────────────────────────────────────────────────────────┐│
//! │  │                      Cloud API (cloud-api crate)                   ││
//! │  │                                                                    ││
//! │  │  Port 50051 │ AuthService │ SyncService │ ConfigService           ││
//! │  └────────────────────────────────────────────────────────────────────┘│
//! └─────────────────────────────────────────────────────────────────────────┘
//! ```

use crate::cloud_auth::{CloudAuth, CloudAuthConfig};
use crate::error::{SyncError, SyncResult};
use crate::proto::{
    sync_service_client::SyncServiceClient,
    config_service_client::ConfigServiceClient,
    health_service_client::HealthServiceClient,
    health_check_response::ServingStatus,
    sync_entity, SyncEntity, GetPendingUpdatesRequest, UploadBatchRequest,
    UploadBatchResponse, GetStoreConfigRequest, GetStoreConfigResponse,
    HealthCheckRequest, Money, Timestamp, Sale, SaleItem, Payment,
    EntityUpdate,
};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio_stream::StreamExt;
use tonic::transport::{Channel, Endpoint};
use tracing::{debug, info, warn};

/// Configuration for the cloud uplink
#[derive(Debug, Clone)]
pub struct CloudUplinkConfig {
    /// Cloud API endpoint URL
    pub cloud_url: String,
    /// Store ID
    pub store_id: String,
    /// Tenant ID
    pub tenant_id: String,
    /// Store's API key
    pub api_key: String,
    /// Device ID
    pub device_id: String,
    /// Device name
    pub device_name: Option<String>,
    /// Enable TLS verification
    pub verify_tls: bool,
    /// Upload batch size
    pub batch_size: usize,
    /// Upload interval
    pub upload_interval: Duration,
    /// Download interval
    pub download_interval: Duration,
    /// Connection timeout
    pub connect_timeout: Duration,
    /// Request timeout
    pub request_timeout: Duration,
}

impl Default for CloudUplinkConfig {
    fn default() -> Self {
        Self {
            cloud_url: "http://localhost:50051".to_string(),
            store_id: String::new(),
            tenant_id: String::new(),
            api_key: String::new(),
            device_id: String::new(),
            device_name: None,
            verify_tls: true,
            batch_size: 100,
            upload_interval: Duration::from_secs(30),
            download_interval: Duration::from_secs(60),
            connect_timeout: Duration::from_secs(10),
            request_timeout: Duration::from_secs(30),
        }
    }
}

/// Cloud uplink client for gRPC communication.
///
/// This is the main interface for PRIMARY nodes to communicate with the cloud.
pub struct CloudUplink {
    config: CloudUplinkConfig,
    auth: Arc<CloudAuth>,
    channel: Option<Channel>,
    connected: Arc<RwLock<bool>>,
}

impl CloudUplink {
    /// Create a new cloud uplink client.
    pub fn new(config: CloudUplinkConfig) -> SyncResult<Self> {
        let auth_config = CloudAuthConfig {
            cloud_url: config.cloud_url.clone(),
            store_id: config.store_id.clone(),
            tenant_id: config.tenant_id.clone(),
            api_key: config.api_key.clone(),
            device_id: config.device_id.clone(),
            device_name: config.device_name.clone(),
            verify_tls: config.verify_tls,
        };

        let auth = Arc::new(CloudAuth::new(auth_config)?);

        Ok(Self {
            config,
            auth,
            channel: None,
            connected: Arc::new(RwLock::new(false)),
        })
    }

    /// Connect to the cloud API.
    pub async fn connect(&mut self) -> SyncResult<()> {
        info!(url = %self.config.cloud_url, "Connecting to cloud API");

        let endpoint = Endpoint::from_shared(self.config.cloud_url.clone())
            .map_err(|e| SyncError::Connection(format!("Invalid endpoint: {}", e)))?
            .connect_timeout(self.config.connect_timeout);

        let channel = endpoint
            .connect()
            .await
            .map_err(|e| SyncError::Connection(format!("Failed to connect: {}", e)))?;

        // Authenticate
        self.auth.authenticate().await?;

        self.channel = Some(channel);
        *self.connected.write().await = true;

        info!("Connected to cloud API");
        Ok(())
    }

    /// Disconnect from the cloud API.
    pub async fn disconnect(&mut self) {
        self.channel = None;
        *self.connected.write().await = false;
        info!("Disconnected from cloud API");
    }

    /// Check if connected to the cloud.
    pub async fn is_connected(&self) -> bool {
        *self.connected.read().await
    }

    /// Get the gRPC channel.
    fn channel(&self) -> SyncResult<Channel> {
        self.channel
            .clone()
            .ok_or_else(|| SyncError::Connection("Not connected to cloud".to_string()))
    }

    /// Upload a batch of sync data to the cloud.
    ///
    /// # Arguments
    /// * `entities` - Vec of sync entities (sales, payments, inventory deltas)
    pub async fn upload_batch(&self, entities: Vec<SyncEntity>) -> SyncResult<UploadBatchResponse> {
        let channel = self.channel()?;
        let token = self.auth.get_access_token().await?;

        let mut client = SyncServiceClient::with_interceptor(
            channel,
            move |mut req: tonic::Request<()>| {
                let token = token.clone();
                req.metadata_mut().insert(
                    "authorization",
                    format!("Bearer {}", token)
                        .parse()
                        .expect("valid header value"),
                );
                Ok(req)
            },
        );

        let batch_id = uuid::Uuid::new_v4().to_string();
        let entity_count = entities.len();

        info!(batch_id = %batch_id, entity_count, "Uploading batch to cloud");

        let request = UploadBatchRequest {
            batch_id: batch_id.clone(),
            store_id: self.config.store_id.clone(),
            device_id: self.config.device_id.clone(),
            entities,
            cursors: vec![], // No cursors to report in this batch
        };

        let response = client
            .upload_batch(request)
            .await
            .map_err(|e| SyncError::Upload(format!("Upload failed: {}", e)))?;

        let ack = response.into_inner();

        info!(
            batch_id = %batch_id,
            success = ack.success,
            synced_count = ack.synced_ids.len(),
            error_count = ack.errors.len(),
            "Upload batch complete"
        );

        Ok(ack)
    }

    /// Download pending updates from the cloud.
    pub async fn download_updates(&self) -> SyncResult<Vec<EntityUpdate>> {
        let channel = self.channel()?;
        let token = self.auth.get_access_token().await?;

        let mut client = SyncServiceClient::with_interceptor(
            channel,
            move |mut req: tonic::Request<()>| {
                let token = token.clone();
                req.metadata_mut().insert(
                    "authorization",
                    format!("Bearer {}", token)
                        .parse()
                        .expect("valid header value"),
                );
                Ok(req)
            },
        );

        info!("Downloading pending updates from cloud");

        let request = GetPendingUpdatesRequest {
            store_id: self.config.store_id.clone(),
            cursor: None,
            limit: self.config.batch_size as i32,
            entity_types: vec![],
        };

        let response = client
            .get_pending_updates(request)
            .await
            .map_err(|e| SyncError::Download(format!("Download failed: {}", e)))?;

        let mut updates = Vec::new();
        let mut stream = response.into_inner();

        while let Some(result) = stream.next().await {
            match result {
                Ok(update) => {
                    debug!(update_id = %update.update_id, "Received update");
                    updates.push(update);
                }
                Err(e) => {
                    warn!(error = %e, "Error receiving update");
                    break;
                }
            }
        }

        info!(count = updates.len(), "Downloaded updates from cloud");
        Ok(updates)
    }

    /// Get store configuration from the cloud.
    pub async fn get_store_config(&self) -> SyncResult<GetStoreConfigResponse> {
        let channel = self.channel()?;
        let token = self.auth.get_access_token().await?;

        let mut client = ConfigServiceClient::with_interceptor(
            channel,
            move |mut req: tonic::Request<()>| {
                let token = token.clone();
                req.metadata_mut().insert(
                    "authorization",
                    format!("Bearer {}", token)
                        .parse()
                        .expect("valid header value"),
                );
                Ok(req)
            },
        );

        let request = GetStoreConfigRequest {
            store_id: self.config.store_id.clone(),
        };

        let response = client
            .get_store_config(request)
            .await
            .map_err(|e| SyncError::Cloud(format!("Failed to get config: {}", e)))?;

        Ok(response.into_inner())
    }

    /// Check cloud health.
    pub async fn health_check(&self) -> SyncResult<bool> {
        let channel = self.channel()?;

        let mut client = HealthServiceClient::new(channel);

        let request = HealthCheckRequest {
            service: String::new(),
        };

        let response = client
            .check(request)
            .await
            .map_err(|e| SyncError::Cloud(format!("Health check failed: {}", e)))?;

        let status = response.into_inner();
        let serving = status.status == ServingStatus::Serving as i32;

        debug!(serving, message = %status.message, "Health check result");
        Ok(serving)
    }
}

// =============================================================================
// Entity Conversion Helpers
// =============================================================================

/// Convert a titan_core::Sale to a proto::SyncEntity.
///
/// # Field Mapping
/// ```text
/// titan_core::Sale          →  proto::Sale
/// ─────────────────────────────────────────
/// id                        →  id
/// tenant_id                 →  store_id
/// device_id                 →  device_id
/// receipt_number            →  receipt_number
/// subtotal_cents            →  subtotal.cents
/// tax_cents                 →  tax_amount.cents
/// discount_cents            →  discount_amount.cents
/// total_cents               →  total.cents
/// status (enum)             →  status (string: DRAFT, COMPLETED, VOIDED)
/// created_at                →  created_at
/// completed_at              →  completed_at
/// ```
pub fn sale_to_entity(sale: &titan_core::Sale) -> SyncEntity {
    // Convert SaleStatus enum to proto string
    let status_str = match sale.status {
        titan_core::SaleStatus::Draft => "DRAFT",
        titan_core::SaleStatus::Completed => "COMPLETED",
        titan_core::SaleStatus::Voided => "VOIDED",
    };

    SyncEntity {
        entity_id: sale.id.clone(),
        entity_type: "SALE".to_string(),
        device_sequence: sale.sync_version,
        created_at: Some(Timestamp {
            value: sale.created_at.to_rfc3339(),
        }),
        data: Some(sync_entity::Data::Sale(Sale {
            id: sale.id.clone(),
            store_id: sale.tenant_id.clone(),
            device_id: sale.device_id.clone(),
            receipt_number: sale.receipt_number.clone(),
            subtotal: Some(Money {
                cents: sale.subtotal_cents,
                currency: "USD".to_string(),
            }),
            tax_amount: Some(Money {
                cents: sale.tax_cents,
                currency: "USD".to_string(),
            }),
            discount_amount: Some(Money {
                cents: sale.discount_cents,
                currency: "USD".to_string(),
            }),
            total: Some(Money {
                cents: sale.total_cents,
                currency: "USD".to_string(),
            }),
            status: status_str.to_string(),
            created_at: Some(Timestamp {
                value: sale.created_at.to_rfc3339(),
            }),
            completed_at: sale.completed_at.as_ref().map(|dt| Timestamp {
                value: dt.to_rfc3339(),
            }),
            items: vec![], // Items are sent separately as SALE_ITEM entities
        })),
    }
}

/// Convert a titan_core::SaleItem to a proto::SyncEntity.
///
/// # Field Mapping
/// ```text
/// titan_core::SaleItem      →  proto::SaleItem
/// ─────────────────────────────────────────────
/// id                        →  id
/// sale_id                   →  sale_id
/// product_id                →  product_id
/// sku_snapshot              →  sku
/// name_snapshot             →  name
/// quantity (i64)            →  quantity (i32)
/// unit_price_cents          →  unit_price.cents
/// line_total_cents          →  line_total.cents
/// tax_cents                 →  tax_amount.cents
/// (no tax_rate_bps)         →  tax_rate_bps = 0
/// ```
pub fn sale_item_to_entity(item: &titan_core::SaleItem) -> SyncEntity {
    SyncEntity {
        entity_id: item.id.clone(),
        entity_type: "SALE_ITEM".to_string(),
        device_sequence: 0,
        created_at: Some(Timestamp {
            value: item.created_at.to_rfc3339(),
        }),
        data: Some(sync_entity::Data::SaleItem(SaleItem {
            id: item.id.clone(),
            sale_id: item.sale_id.clone(),
            product_id: item.product_id.clone(),
            sku: item.sku_snapshot.clone(),
            name: item.name_snapshot.clone(),
            quantity: item.quantity as i32,
            unit_price: Some(Money {
                cents: item.unit_price_cents,
                currency: "USD".to_string(),
            }),
            line_total: Some(Money {
                cents: item.line_total_cents,
                currency: "USD".to_string(),
            }),
            tax_amount: Some(Money {
                cents: item.tax_cents,
                currency: "USD".to_string(),
            }),
            tax_rate_bps: 0, // Not stored in SaleItem, would need to look up from Product
        })),
    }
}

/// Convert a titan_core::Payment to a proto::SyncEntity.
///
/// # Field Mapping
/// ```text
/// titan_core::Payment       →  proto::Payment
/// ─────────────────────────────────────────────
/// id                        →  id
/// sale_id                   →  sale_id
/// (none)                    →  store_id (empty, set by cloud)
/// method (enum)             →  method (string: CASH, EXTERNAL_CARD)
/// amount_cents              →  amount.cents
/// change_cents              →  change_given.cents
/// reference                 →  reference
/// (none)                    →  authorization_code (empty)
/// created_at                →  created_at
/// ```
pub fn payment_to_entity(payment: &titan_core::Payment) -> SyncEntity {
    // Convert PaymentMethod enum to proto string
    let method_str = match payment.method {
        titan_core::PaymentMethod::Cash => "CASH",
        titan_core::PaymentMethod::ExternalCard => "EXTERNAL_CARD",
    };

    SyncEntity {
        entity_id: payment.id.clone(),
        entity_type: "PAYMENT".to_string(),
        device_sequence: 0,
        created_at: Some(Timestamp {
            value: payment.created_at.to_rfc3339(),
        }),
        data: Some(sync_entity::Data::Payment(Payment {
            id: payment.id.clone(),
            sale_id: payment.sale_id.clone(),
            store_id: String::new(), // Will be set by cloud from JWT claims
            method: method_str.to_string(),
            amount: Some(Money {
                cents: payment.amount_cents,
                currency: "USD".to_string(),
            }),
            change_given: Some(Money {
                cents: payment.change_cents.unwrap_or(0),
                currency: "USD".to_string(),
            }),
            reference: payment.reference.clone().unwrap_or_default(),
            authorization_code: String::new(),
            created_at: Some(Timestamp {
                value: payment.created_at.to_rfc3339(),
            }),
        })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = CloudUplinkConfig::default();
        assert_eq!(config.cloud_url, "http://localhost:50051");
        assert_eq!(config.batch_size, 100);
    }
}
