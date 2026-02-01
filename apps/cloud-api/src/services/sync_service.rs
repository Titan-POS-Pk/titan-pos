//! Sync gRPC service implementation.
//!
//! Handles bidirectional data synchronization between Store Hubs and Cloud.

use std::pin::Pin;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use tokio::sync::mpsc;
use tokio_stream::{wrappers::ReceiverStream, Stream, StreamExt};
use tonic::{Request, Response, Status, Streaming};
use tracing::{debug, error, info, warn};

use crate::auth::{extract_bearer_token, JwtManager};
use crate::db::{InventoryDeltaRecord, PaymentRecord, SaleItemRecord, SaleRecord};
use crate::proto::{
    sync_service_server::SyncService,
    AcknowledgeUpdatesRequest, AcknowledgeUpdatesResponse,
    EntityUpdate, GetPendingUpdatesRequest,
    GetSyncStatusRequest, GetSyncStatusResponse,
    ReportCursorRequest, ReportCursorResponse,
    SyncCursor, SyncEntity, SyncError,
    UploadBatchRequest, UploadBatchResponse,
    Timestamp as ProtoTimestamp,
};
use crate::AppState;

/// Sync service implementation.
pub struct SyncServiceImpl {
    state: Arc<AppState>,
    jwt_manager: JwtManager,
}

impl SyncServiceImpl {
    /// Create a new sync service.
    pub fn new(state: Arc<AppState>) -> Self {
        let jwt_manager = JwtManager::new(
            state.config.jwt_secret.clone(),
            state.config.jwt_access_lifetime_secs,
            state.config.jwt_refresh_lifetime_secs,
        );
        
        SyncServiceImpl { state, jwt_manager }
    }

    /// Authenticate a request from metadata.
    fn authenticate(&self, request: &Request<impl std::any::Any>) -> Result<AuthContext, Status> {
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

        Ok(AuthContext {
            store_id: claims.sub,
            tenant_id: claims.tenant_id,
            device_id: claims.device_id,
        })
    }

    /// Process a single sync entity.
    async fn process_entity(
        &self,
        auth: &AuthContext,
        entity: &SyncEntity,
    ) -> Result<(), SyncError> {
        match entity.entity_type.as_str() {
            "SALE" => {
                if let Some(crate::proto::sync_entity::Data::Sale(sale)) = &entity.data {
                    self.process_sale(auth, sale).await?;
                }
            }
            "SALE_ITEM" => {
                if let Some(crate::proto::sync_entity::Data::SaleItem(item)) = &entity.data {
                    self.process_sale_item(auth, item).await?;
                }
            }
            "PAYMENT" => {
                if let Some(crate::proto::sync_entity::Data::Payment(payment)) = &entity.data {
                    self.process_payment(auth, payment).await?;
                }
            }
            "INVENTORY_DELTA" => {
                if let Some(crate::proto::sync_entity::Data::InventoryDelta(delta)) = &entity.data {
                    self.process_inventory_delta(auth, delta).await?;
                }
            }
            other => {
                return Err(SyncError {
                    entity_id: entity.entity_id.clone(),
                    error_code: "UNKNOWN_ENTITY_TYPE".to_string(),
                    error_message: format!("Unknown entity type: {}", other),
                    retryable: false,
                });
            }
        }
        
        Ok(())
    }

    /// Process a sale record.
    async fn process_sale(
        &self,
        auth: &AuthContext,
        sale: &crate::proto::Sale,
    ) -> Result<(), SyncError> {
        let created_at = parse_timestamp(&sale.created_at)?;
        let completed_at = if let Some(ref ts) = sale.completed_at {
            Some(parse_timestamp(&Some(ts.clone()))?)
        } else {
            None
        };

        let record = SaleRecord {
            id: sale.id.clone(),
            store_id: auth.store_id.clone(),
            device_id: sale.device_id.clone(),
            tenant_id: auth.tenant_id.clone(),
            receipt_number: sale.receipt_number.clone(),
            subtotal_cents: sale.subtotal.as_ref().map(|m| m.cents).unwrap_or(0),
            tax_amount_cents: sale.tax_amount.as_ref().map(|m| m.cents).unwrap_or(0),
            discount_amount_cents: sale.discount_amount.as_ref().map(|m| m.cents).unwrap_or(0),
            total_cents: sale.total.as_ref().map(|m| m.cents).unwrap_or(0),
            status: sale.status.clone(),
            created_at,
            completed_at,
        };

        self.state.db.insert_sale(&record).await.map_err(|e| SyncError {
            entity_id: sale.id.clone(),
            error_code: "DB_ERROR".to_string(),
            error_message: e.to_string(),
            retryable: true,
        })?;

        Ok(())
    }

    /// Process a sale item record.
    async fn process_sale_item(
        &self,
        _auth: &AuthContext,
        item: &crate::proto::SaleItem,
    ) -> Result<(), SyncError> {
        let record = SaleItemRecord {
            id: item.id.clone(),
            sale_id: item.sale_id.clone(),
            product_id: item.product_id.clone(),
            sku: item.sku.clone(),
            name: item.name.clone(),
            quantity: item.quantity,
            unit_price_cents: item.unit_price.as_ref().map(|m| m.cents).unwrap_or(0),
            line_total_cents: item.line_total.as_ref().map(|m| m.cents).unwrap_or(0),
            tax_amount_cents: item.tax_amount.as_ref().map(|m| m.cents).unwrap_or(0),
            tax_rate_bps: item.tax_rate_bps,
        };

        self.state.db.insert_sale_item(&record).await.map_err(|e| SyncError {
            entity_id: item.id.clone(),
            error_code: "DB_ERROR".to_string(),
            error_message: e.to_string(),
            retryable: true,
        })?;

        Ok(())
    }

    /// Process a payment record.
    async fn process_payment(
        &self,
        auth: &AuthContext,
        payment: &crate::proto::Payment,
    ) -> Result<(), SyncError> {
        let created_at = parse_timestamp(&payment.created_at)?;

        let record = PaymentRecord {
            id: payment.id.clone(),
            sale_id: payment.sale_id.clone(),
            store_id: auth.store_id.clone(),
            tenant_id: auth.tenant_id.clone(),
            method: payment.method.clone(),
            amount_cents: payment.amount.as_ref().map(|m| m.cents).unwrap_or(0),
            change_given_cents: payment.change_given.as_ref().map(|m| m.cents).unwrap_or(0),
            reference: if payment.reference.is_empty() { None } else { Some(payment.reference.clone()) },
            authorization_code: if payment.authorization_code.is_empty() { None } else { Some(payment.authorization_code.clone()) },
            created_at,
        };

        self.state.db.insert_payment(&record).await.map_err(|e| SyncError {
            entity_id: payment.id.clone(),
            error_code: "DB_ERROR".to_string(),
            error_message: e.to_string(),
            retryable: true,
        })?;

        Ok(())
    }

    /// Process an inventory delta (CRDT).
    async fn process_inventory_delta(
        &self,
        auth: &AuthContext,
        delta: &crate::proto::InventoryDelta,
    ) -> Result<(), SyncError> {
        let created_at = parse_timestamp(&delta.created_at)?;

        let record = InventoryDeltaRecord {
            id: delta.id.clone(),
            store_id: auth.store_id.clone(),
            device_id: delta.device_id.clone(),
            tenant_id: auth.tenant_id.clone(),
            product_id: delta.product_id.clone(),
            delta: delta.delta,
            reason: delta.reason.clone(),
            reference_id: if delta.reference_id.is_empty() { None } else { Some(delta.reference_id.clone()) },
            created_at,
        };

        self.state.db.apply_inventory_delta(&record).await.map_err(|e| SyncError {
            entity_id: delta.id.clone(),
            error_code: "DB_ERROR".to_string(),
            error_message: e.to_string(),
            retryable: true,
        })?;

        Ok(())
    }
}

#[tonic::async_trait]
impl SyncService for SyncServiceImpl {
    /// Upload a batch of entities.
    async fn upload_batch(
        &self,
        request: Request<UploadBatchRequest>,
    ) -> Result<Response<UploadBatchResponse>, Status> {
        let auth = self.authenticate(&request)?;
        let req = request.into_inner();

        info!(
            store_id = %auth.store_id,
            batch_id = %req.batch_id,
            entity_count = req.entities.len(),
            "Processing upload batch"
        );

        let mut synced_ids = Vec::new();
        let mut errors = Vec::new();

        for entity in &req.entities {
            match self.process_entity(&auth, entity).await {
                Ok(()) => {
                    synced_ids.push(entity.entity_id.clone());
                }
                Err(sync_error) => {
                    warn!(
                        entity_id = %sync_error.entity_id,
                        error = %sync_error.error_message,
                        "Failed to process entity"
                    );
                    errors.push(sync_error);
                }
            }
        }

        // Update cursors
        for cursor in &req.cursors {
            if let Err(e) = self.state.db
                .update_sync_cursor(&auth.store_id, &cursor.stream, cursor.position)
                .await
            {
                warn!(stream = %cursor.stream, ?e, "Failed to update cursor");
            }
        }

        let success = errors.is_empty();
        
        info!(
            store_id = %auth.store_id,
            batch_id = %req.batch_id,
            synced = synced_ids.len(),
            failed = errors.len(),
            "Batch processing complete"
        );

        Ok(Response::new(UploadBatchResponse {
            batch_id: req.batch_id,
            success,
            synced_ids,
            errors,
            new_cursor: None, // Will be set by cursor tracking
        }))
    }

    type StreamUploadStream = Pin<Box<dyn Stream<Item = Result<UploadBatchResponse, Status>> + Send>>;

    /// Stream upload for large batches.
    async fn stream_upload(
        &self,
        request: Request<Streaming<UploadBatchRequest>>,
    ) -> Result<Response<Self::StreamUploadStream>, Status> {
        let auth = self.authenticate(&request)?;
        let mut stream = request.into_inner();

        let state = self.state.clone();
        let jwt_manager = JwtManager::new(
            state.config.jwt_secret.clone(),
            state.config.jwt_access_lifetime_secs,
            state.config.jwt_refresh_lifetime_secs,
        );
        
        let (tx, rx) = mpsc::channel(32);

        tokio::spawn(async move {
            while let Some(result) = stream.next().await {
                let response = match result {
                    Ok(req) => {
                        // Process each batch in the stream
                        let mut synced_ids = Vec::new();
                        let mut errors = Vec::new();

                        for entity in &req.entities {
                            // Create a temporary service for processing
                            let service = SyncServiceImpl {
                                state: state.clone(),
                                jwt_manager: JwtManager::new(
                                    state.config.jwt_secret.clone(),
                                    state.config.jwt_access_lifetime_secs,
                                    state.config.jwt_refresh_lifetime_secs,
                                ),
                            };
                            
                            match service.process_entity(&auth, entity).await {
                                Ok(()) => synced_ids.push(entity.entity_id.clone()),
                                Err(e) => errors.push(e),
                            }
                        }

                        Ok(UploadBatchResponse {
                            batch_id: req.batch_id,
                            success: errors.is_empty(),
                            synced_ids,
                            errors,
                            new_cursor: None,
                        })
                    }
                    Err(e) => Err(e),
                };

                if tx.send(response).await.is_err() {
                    break;
                }
            }
        });

        let output_stream = ReceiverStream::new(rx);
        Ok(Response::new(Box::pin(output_stream)))
    }

    type GetPendingUpdatesStream = Pin<Box<dyn Stream<Item = Result<EntityUpdate, Status>> + Send>>;

    /// Get pending downloads for a store.
    async fn get_pending_updates(
        &self,
        request: Request<GetPendingUpdatesRequest>,
    ) -> Result<Response<Self::GetPendingUpdatesStream>, Status> {
        let auth = self.authenticate(&request)?;
        let req = request.into_inner();

        let since_version = req.cursor.as_ref().map(|c| c.position).unwrap_or(0);
        let limit = req.limit;

        info!(
            store_id = %auth.store_id,
            since_version = since_version,
            "Fetching pending updates"
        );

        // Fetch pending product updates
        let products = self.state.db
            .get_pending_product_updates(&auth.store_id, since_version, limit)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        let (tx, rx) = mpsc::channel(32);

        tokio::spawn(async move {
            for product in products {
                let update = EntityUpdate {
                    update_id: format!("product-{}-{}", product.id, product.version),
                    entity_type: "PRODUCT".to_string(),
                    operation: "UPDATE".to_string(),
                    data: Some(crate::proto::entity_update::Data::Product(
                        crate::proto::Product {
                            id: product.id,
                            sku: product.sku,
                            name: product.name,
                            barcode: product.barcode.unwrap_or_default(),
                            price: Some(crate::proto::Money {
                                cents: product.price_cents,
                                currency: "USD".to_string(),
                            }),
                            cost: product.cost_cents.map(|c| crate::proto::Money {
                                cents: c,
                                currency: "USD".to_string(),
                            }),
                            tax_rate_id: product.tax_rate_id.unwrap_or_default(),
                            tax_rate_bps: product.tax_rate_bps,
                            track_inventory: product.track_inventory,
                            current_stock: product.current_stock.unwrap_or(0),
                            low_stock_threshold: product.low_stock_threshold.unwrap_or(0),
                            is_active: product.is_active,
                            category: product.category.unwrap_or_default(),
                            department: product.department.unwrap_or_default(),
                            created_at: Some(ProtoTimestamp {
                                value: product.created_at.to_rfc3339(),
                            }),
                            updated_at: Some(ProtoTimestamp {
                                value: product.updated_at.to_rfc3339(),
                            }),
                            version: product.version,
                        },
                    )),
                    version: product.version,
                    updated_at: Some(ProtoTimestamp {
                        value: product.updated_at.to_rfc3339(),
                    }),
                };

                if tx.send(Ok(update)).await.is_err() {
                    break;
                }
            }
        });

        let output_stream = ReceiverStream::new(rx);
        Ok(Response::new(Box::pin(output_stream)))
    }

    /// Acknowledge receipt of updates.
    async fn acknowledge_updates(
        &self,
        request: Request<AcknowledgeUpdatesRequest>,
    ) -> Result<Response<AcknowledgeUpdatesResponse>, Status> {
        let auth = self.authenticate(&request)?;
        let req = request.into_inner();

        info!(
            store_id = %auth.store_id,
            update_count = req.update_ids.len(),
            "Acknowledging updates"
        );

        // Update cursor if provided
        if let Some(cursor) = req.new_cursor {
            self.state.db
                .update_sync_cursor(&auth.store_id, &cursor.stream, cursor.position)
                .await
                .map_err(|e| Status::internal(e.to_string()))?;
        }

        Ok(Response::new(AcknowledgeUpdatesResponse { success: true }))
    }

    /// Get current sync status.
    async fn get_sync_status(
        &self,
        request: Request<GetSyncStatusRequest>,
    ) -> Result<Response<GetSyncStatusResponse>, Status> {
        let auth = self.authenticate(&request)?;

        // Get cursor positions
        let upload_cursor = self.state.db
            .get_sync_cursor(&auth.store_id, "upload")
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        let download_cursor = self.state.db
            .get_sync_cursor(&auth.store_id, "download")
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        let cursors = vec![
            SyncCursor {
                position: upload_cursor.unwrap_or(0),
                stream: "upload".to_string(),
                updated_at: None,
            },
            SyncCursor {
                position: download_cursor.unwrap_or(0),
                stream: "download".to_string(),
                updated_at: None,
            },
        ];

        Ok(Response::new(GetSyncStatusResponse {
            connected: true,
            last_sync: Some(ProtoTimestamp {
                value: Utc::now().to_rfc3339(),
            }),
            pending_uploads: 0,
            pending_downloads: 0,
            cursors,
            health_status: "HEALTHY".to_string(),
            health_message: String::new(),
        }))
    }

    /// Report sync cursor position.
    async fn report_cursor(
        &self,
        request: Request<ReportCursorRequest>,
    ) -> Result<Response<ReportCursorResponse>, Status> {
        let auth = self.authenticate(&request)?;
        let req = request.into_inner();

        self.state.db
            .update_sync_cursor(&auth.store_id, &req.stream, req.position)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        let server_position = self.state.db
            .get_sync_cursor(&auth.store_id, &req.stream)
            .await
            .map_err(|e| Status::internal(e.to_string()))?
            .unwrap_or(req.position);

        Ok(Response::new(ReportCursorResponse {
            success: true,
            server_position,
        }))
    }
}

// =============================================================================
// Helper Types
// =============================================================================

/// Authentication context extracted from JWT.
struct AuthContext {
    store_id: String,
    tenant_id: String,
    device_id: String,
}

/// Parse a proto timestamp to DateTime<Utc>.
fn parse_timestamp(ts: &Option<ProtoTimestamp>) -> Result<DateTime<Utc>, SyncError> {
    let ts = ts.as_ref().ok_or_else(|| SyncError {
        entity_id: String::new(),
        error_code: "INVALID_TIMESTAMP".to_string(),
        error_message: "Missing timestamp".to_string(),
        retryable: false,
    })?;

    DateTime::parse_from_rfc3339(&ts.value)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| SyncError {
            entity_id: String::new(),
            error_code: "INVALID_TIMESTAMP".to_string(),
            error_message: format!("Invalid timestamp format: {}", e),
            retryable: false,
        })
}
