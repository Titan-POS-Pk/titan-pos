//! # Sync Agent
//!
//! Main orchestrator for the sync engine. Coordinates outbox processing,
//! transport management, and inbound updates.
//!
//! ## Agent Architecture
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                        SyncAgent Architecture                           │
//! │                                                                         │
//! │  ┌──────────────────────────────────────────────────────────────────┐  │
//! │  │                         SyncAgent                                │  │
//! │  │                                                                  │  │
//! │  │  • Spawns and manages all sync components                        │  │
//! │  │  • Handles connection state transitions                          │  │
//! │  │  • Routes messages to appropriate handlers                       │  │
//! │  │  • Emits status events to Tauri frontend                         │  │
//! │  └────────────────────────────┬─────────────────────────────────────┘  │
//! │                               │                                         │
//! │         ┌─────────────────────┼─────────────────────┐                  │
//! │         ▼                     ▼                     ▼                   │
//! │  ┌────────────────┐  ┌────────────────┐  ┌────────────────────────┐    │
//! │  │   Transport    │  │OutboxProcessor │  │   InboundHandler       │    │
//! │  │   (WebSocket)  │  │                │  │                        │    │
//! │  │                │  │ Uploads local  │  │ Applies remote         │    │
//! │  │ Manages WS     │  │ changes to hub │  │ changes locally        │    │
//! │  │ connection     │  │                │  │                        │    │
//! │  └────────────────┘  └────────────────┘  └────────────────────────┘    │
//! │                                                                         │
//! │  STATUS EVENTS (to Tauri):                                             │
//! │  ────────────────────────                                              │
//! │  "sync://status"   - { state: "connected", hub: "..." }                │
//! │  "sync://progress" - { pending: 5, synced: 100 }                       │
//! │  "sync://error"    - { message: "Connection failed", retryable: true } │
//! └─────────────────────────────────────────────────────────────────────────┘
//! ```

use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, error, info, warn};

use titan_db::Database;

use crate::config::{SyncConfig, SyncMode};
use crate::error::{SyncError, SyncResult};
use crate::inbound::{InboundHandler, InboundHandlerHandle};
use crate::outbox::{OutboxProcessor, OutboxProcessorHandle};
use crate::protocol::SyncMessage;
use crate::transport::{ConnectionState, Transport, TransportConfig, TransportHandle};

// =============================================================================
// Sync Status
// =============================================================================

/// Current sync status for external queries.
#[derive(Debug, Clone)]
pub struct SyncStatus {
    /// Current connection state.
    pub connection_state: ConnectionState,

    /// Whether currently connected to hub.
    pub is_connected: bool,

    /// URL of the connected hub (if any).
    pub hub_url: Option<String>,

    /// Number of pending outbox entries.
    pub pending_count: i64,

    /// Last successful sync timestamp (ISO8601).
    pub last_sync: Option<String>,

    /// Last error message (if any).
    pub last_error: Option<String>,

    /// Sync mode.
    pub mode: SyncMode,
}

impl Default for SyncStatus {
    fn default() -> Self {
        SyncStatus {
            connection_state: ConnectionState::Disconnected,
            is_connected: false,
            hub_url: None,
            pending_count: 0,
            last_sync: None,
            last_error: None,
            mode: SyncMode::Auto,
        }
    }
}

// =============================================================================
// Event Emitter Trait
// =============================================================================

/// Trait for emitting sync events (implemented by Tauri integration).
pub trait SyncEventEmitter: Send + Sync {
    /// Emits a sync status change event.
    fn emit_status(&self, status: &SyncStatus);

    /// Emits a sync progress event.
    fn emit_progress(&self, pending: i64, synced: i64);

    /// Emits a sync error event.
    fn emit_error(&self, message: &str, retryable: bool);
    
    /// Emits an inventory update event.
    /// Called when SECONDARY receives inventory updates from PRIMARY.
    fn emit_inventory_update(&self, product_ids: Vec<String>, reason: &str);
}

/// No-op event emitter for testing.
pub struct NoOpEmitter;

impl SyncEventEmitter for NoOpEmitter {
    fn emit_status(&self, _status: &SyncStatus) {}
    fn emit_progress(&self, _pending: i64, _synced: i64) {}
    fn emit_error(&self, _message: &str, _retryable: bool) {}
    fn emit_inventory_update(&self, _product_ids: Vec<String>, _reason: &str) {}
}

// =============================================================================
// Sync Agent
// =============================================================================

/// Main sync agent that orchestrates all sync operations.
pub struct SyncAgent {
    /// Sync configuration.
    config: Arc<SyncConfig>,

    /// Database connection.
    db: Arc<Database>,

    /// Current sync status.
    status: Arc<RwLock<SyncStatus>>,

    /// Event emitter for frontend notifications.
    emitter: Arc<dyn SyncEventEmitter>,

    /// Shutdown sender.
    shutdown_tx: Option<mpsc::Sender<()>>,

    /// Transport handle (set after start).
    transport: Option<TransportHandle>,

    /// Outbox processor handle.
    outbox_handle: Option<OutboxProcessorHandle>,

    /// Inbound handler handle.
    inbound_handle: Option<InboundHandlerHandle>,
}

impl SyncAgent {
    /// Creates a new sync agent.
    pub fn new(config: SyncConfig, db: Arc<Database>) -> Self {
        Self::with_emitter(config, db, Arc::new(NoOpEmitter))
    }

    /// Creates a new sync agent with a custom event emitter.
    pub fn with_emitter(
        config: SyncConfig,
        db: Arc<Database>,
        emitter: Arc<dyn SyncEventEmitter>,
    ) -> Self {
        let status = SyncStatus {
            mode: config.sync.mode,
            ..Default::default()
        };

        SyncAgent {
            config: Arc::new(config),
            db,
            status: Arc::new(RwLock::new(status)),
            emitter,
            shutdown_tx: None,
            transport: None,
            outbox_handle: None,
            inbound_handle: None,
        }
    }

    /// Returns the current sync status.
    pub async fn status(&self) -> SyncStatus {
        self.status.read().await.clone()
    }

    /// Starts the sync agent.
    ///
    /// This spawns background tasks for transport, outbox processing, and
    /// inbound handling. The agent continues running until shutdown is called.
    ///
    /// # Returns
    /// A `SyncAgentHandle` that must be kept alive to keep the agent running.
    /// Dropping the handle will trigger agent shutdown.
    pub async fn start(&mut self) -> SyncResult<SyncAgentHandle> {
        // Check if sync is enabled
        if !self.config.is_sync_enabled() {
            info!("Sync is disabled (mode: offline)");
            // Return a dummy handle - the agent isn't actually running
            let (shutdown_tx, _) = mpsc::channel(1);
            return Ok(SyncAgentHandle::new(shutdown_tx, self.status.clone()));
        }

        // Validate configuration
        self.config.validate()?;

        // Get hub URL
        let hub_url = match self.config.hub_url() {
            Some(url) => url.to_string(),
            None => {
                // In future milestones, we'd start discovery here
                // For now, require explicit hub URL
                warn!("No hub URL configured, sync will not start");
                return Err(SyncError::InvalidConfig(
                    "Hub URL required for sync".into(),
                ));
            }
        };

        info!(
            device_id = %self.config.device_id(),
            hub_url = %hub_url,
            mode = %self.config.mode(),
            "Starting sync agent"
        );

        // Create transport config
        let transport_config = TransportConfig {
            url: hub_url.clone(),
            connect_timeout: std::time::Duration::from_secs(self.config.sync.connect_timeout_secs),
            initial_backoff: std::time::Duration::from_millis(self.config.sync.initial_backoff_ms),
            max_backoff: std::time::Duration::from_secs(self.config.sync.max_backoff_secs),
            max_retries: self.config.sync.max_retries,
            ..Default::default()
        };

        // Spawn transport
        let (transport_handle, incoming_rx) = Transport::spawn(transport_config);
        self.transport = Some(transport_handle.clone());

        // Create outbox processor
        let (outbox_processor, outbox_handle) = OutboxProcessor::new(
            self.db.clone(),
            self.config.clone(),
            transport_handle.clone(),
        );
        self.outbox_handle = Some(outbox_handle.clone());

        // Create inbound handler
        let (inbound_handler, inbound_handle) = InboundHandler::new(
            self.db.clone(),
            self.config.clone(),
            transport_handle.clone(),
            self.emitter.clone(),
        );
        self.inbound_handle = Some(inbound_handle.clone());

        // Create shutdown channel
        // Clone the sender because we need to:
        // 1. Store one copy in self.shutdown_tx for the agent's own shutdown() method
        // 2. Return one copy in the SyncAgentHandle
        let (shutdown_tx, shutdown_rx) = mpsc::channel(1);
        let shutdown_tx_for_handle = shutdown_tx.clone();
        self.shutdown_tx = Some(shutdown_tx);

        // Spawn background tasks
        tokio::spawn(outbox_processor.run());
        tokio::spawn(inbound_handler.run());

        // Spawn task to send Hello once connected
        // This is needed because the hub waits for Hello before sending Welcome
        let config_for_hello = self.config.clone();
        let transport_for_hello = transport_handle.clone();
        tokio::spawn(async move {
            // Wait for connection to be established
            loop {
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                if transport_for_hello.is_connected().await {
                    // Send Hello message
                    let hello = SyncMessage::hello(
                        config_for_hello.device_id(),
                        &config_for_hello.device.name,
                        config_for_hello.store_id(),
                        config_for_hello.device.priority,
                    );

                    info!("Transport connected, sending Hello message");
                    if let Err(e) = transport_for_hello.send(hello).await {
                        error!(?e, "Failed to send Hello");
                    } else {
                        info!("Hello message sent successfully");
                    }
                    break;
                }
            }
        });

        // Spawn message router
        let config = self.config.clone();
        let status = self.status.clone();
        let emitter = self.emitter.clone();
        let transport_for_router = transport_handle.clone();

        tokio::spawn(Self::message_router(
            config,
            status,
            emitter,
            incoming_rx,
            transport_for_router,
            outbox_handle,
            inbound_handle,
            shutdown_rx,
        ));

        // Update status
        {
            let mut s = self.status.write().await;
            s.hub_url = Some(hub_url);
        }

        info!("Sync agent started");
        
        // Return handle - CRITICAL: caller MUST keep this alive!
        // Dropping the handle will trigger shutdown via channel closure.
        // Include transport handle for SECONDARY to send inventory deltas
        Ok(SyncAgentHandle::new_with_transport(
            shutdown_tx_for_handle,
            self.status.clone(),
            transport_handle,
        ))
    }

    /// Stops the sync agent gracefully.
    pub async fn shutdown(&mut self) -> SyncResult<()> {
        info!("Shutting down sync agent");

        // Send shutdown signal
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(()).await;
        }

        // Shutdown components
        if let Some(ref handle) = self.outbox_handle {
            let _ = handle.shutdown().await;
        }

        if let Some(ref handle) = self.inbound_handle {
            let _ = handle.shutdown().await;
        }

        if let Some(ref handle) = self.transport {
            let _ = handle.shutdown().await;
        }

        // Update status
        {
            let mut s = self.status.write().await;
            s.connection_state = ConnectionState::Disconnected;
            s.is_connected = false;
        }

        info!("Sync agent stopped");
        Ok(())
    }

    /// Main message router loop.
    async fn message_router(
        config: Arc<SyncConfig>,
        status: Arc<RwLock<SyncStatus>>,
        emitter: Arc<dyn SyncEventEmitter>,
        mut incoming_rx: mpsc::Receiver<SyncMessage>,
        transport: TransportHandle,
        outbox_handle: OutboxProcessorHandle,
        inbound_handle: InboundHandlerHandle,
        mut shutdown_rx: mpsc::Receiver<()>,
    ) {
        let mut handshake_done = false;

        loop {
            tokio::select! {
                Some(msg) = incoming_rx.recv() => {
                    // Update connection status
                    if transport.is_connected().await {
                        let mut s = status.write().await;
                        s.connection_state = ConnectionState::Connected;
                        s.is_connected = true;
                    }

                    match msg {
                        SyncMessage::Welcome(welcome) => {
                            // Handshake complete
                            info!(
                                store_id = %welcome.store_id,
                                term = welcome.election_term,
                                "Handshake complete"
                            );
                            handshake_done = true;

                            // Update status
                            let s = status.read().await.clone();
                            emitter.emit_status(&s);
                        }

                        SyncMessage::BatchAck(ack) => {
                            // Route to outbox processor
                            if let Err(e) = outbox_handle.handle_ack(SyncMessage::BatchAck(ack)).await {
                                error!(?e, "Failed to route batch ack");
                            }
                        }

                        SyncMessage::EntityUpdate(update) => {
                            // Route to inbound handler
                            if let Err(e) = inbound_handle.handle_update(SyncMessage::EntityUpdate(update)).await {
                                error!(?e, "Failed to route entity update");
                            }
                        }
                        
                        SyncMessage::InventoryUpdate(update) => {
                            // Route inventory updates to inbound handler
                            // This applies the stock delta from PRIMARY to SECONDARY's local database
                            info!(
                                product_id = %update.product_id,
                                delta = update.delta_quantity,
                                source = %update.source_device_id,
                                "Received InventoryUpdate from PRIMARY"
                            );
                            if let Err(e) = inbound_handle.handle_inventory_update(update).await {
                                error!(?e, "Failed to apply inventory update");
                            }
                        }

                        SyncMessage::Ping { .. } => {
                            // Send pong (handled by transport layer, but log it)
                            debug!("Received ping");
                        }

                        SyncMessage::Pong { .. } => {
                            debug!("Received pong");
                        }

                        SyncMessage::Error { code, message: msg_text } => {
                            // Handle error from hub
                            warn!(code = %code, message = %msg_text, "Received error from hub");
                            let mut s = status.write().await;
                            s.last_error = Some(format!("{}: {}", code, msg_text));
                            emitter.emit_error(&format!("{}: {}", code, msg_text), true);
                        }

                        other => {
                            debug!(?other, "Unhandled message type");
                        }
                    }

                    // Send Hello if connected but not handshaked
                    if transport.is_connected().await && !handshake_done {
                        let hello = SyncMessage::hello(
                            config.device_id(),
                            &config.device.name,
                            config.store_id(),
                            config.device.priority,
                        );

                        if let Err(e) = transport.send(hello).await {
                            error!(?e, "Failed to send Hello");
                        } else {
                            debug!("Sent Hello message");
                        }
                    }
                }

                _ = shutdown_rx.recv() => {
                    info!("Message router received shutdown");
                    break;
                }
            }
        }

        info!("Message router stopped");
    }
}

// =============================================================================
// Agent Handle (for external control)
// =============================================================================

/// Handle for controlling a running SyncAgent from outside.
///
/// This is used by the Tauri app to control the sync agent without
/// needing direct access to the agent instance.
#[derive(Clone)]
pub struct SyncAgentHandle {
    /// Shutdown sender.
    shutdown_tx: mpsc::Sender<()>,

    /// Status accessor.
    status: Arc<RwLock<SyncStatus>>,
    
    /// Transport handle for sending messages (SECONDARY mode).
    transport: Option<TransportHandle>,
}

impl SyncAgentHandle {
    /// Creates a new handle from agent internals.
    /// 
    /// # IMPORTANT
    /// This handle must be kept alive for the sync agent to continue running.
    /// When all handles are dropped, the shutdown channel closes and the agent
    /// will shut down gracefully.
    pub fn new(
        shutdown_tx: mpsc::Sender<()>,
        status: Arc<RwLock<SyncStatus>>,
    ) -> Self {
        SyncAgentHandle {
            shutdown_tx,
            status,
            transport: None,
        }
    }
    
    /// Creates a new handle with transport access (for SECONDARY mode).
    pub fn new_with_transport(
        shutdown_tx: mpsc::Sender<()>,
        status: Arc<RwLock<SyncStatus>>,
        transport: TransportHandle,
    ) -> Self {
        SyncAgentHandle {
            shutdown_tx,
            status,
            transport: Some(transport),
        }
    }
    
    /// Sends an inventory delta to the hub (SECONDARY mode only).
    /// 
    /// This is used when SECONDARY makes a sale and needs to notify PRIMARY
    /// about the stock change so it can be broadcast to other secondaries.
    pub async fn send_inventory_delta(
        &self,
        product_id: &str,
        sku: &str,
        delta_qty: i32,
    ) -> SyncResult<bool> {
        if let Some(ref transport) = self.transport {
            let delta = crate::protocol::InventoryDelta {
                product_id: product_id.to_string(),
                sku: sku.to_string(),
                delta_quantity: delta_qty,
                timestamp: chrono::Utc::now().to_rfc3339(),
            };
            
            transport.send(SyncMessage::InventoryDelta(delta)).await?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Gets the current sync status.
    pub async fn status(&self) -> SyncStatus {
        self.status.read().await.clone()
    }

    /// Signals the agent to shut down gracefully.
    pub async fn shutdown(&self) {
        let _ = self.shutdown_tx.send(()).await;
    }
}

// =============================================================================
// Builder Pattern
// =============================================================================

/// Builder for creating SyncAgent with options.
pub struct SyncAgentBuilder {
    config: SyncConfig,
    db: Option<Arc<Database>>,
    emitter: Option<Arc<dyn SyncEventEmitter>>,
}

impl SyncAgentBuilder {
    /// Creates a new builder with the given config.
    pub fn new(config: SyncConfig) -> Self {
        SyncAgentBuilder {
            config,
            db: None,
            emitter: None,
        }
    }

    /// Sets the database connection.
    pub fn with_database(mut self, db: Arc<Database>) -> Self {
        self.db = Some(db);
        self
    }

    /// Sets the event emitter.
    pub fn with_emitter(mut self, emitter: Arc<dyn SyncEventEmitter>) -> Self {
        self.emitter = Some(emitter);
        self
    }

    /// Builds the SyncAgent.
    pub fn build(self) -> SyncResult<SyncAgent> {
        let db = self
            .db
            .ok_or_else(|| SyncError::InvalidConfig("Database required".into()))?;

        let emitter = self.emitter.unwrap_or_else(|| Arc::new(NoOpEmitter));

        Ok(SyncAgent::with_emitter(self.config, db, emitter))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sync_status_default() {
        let status = SyncStatus::default();
        assert_eq!(status.connection_state, ConnectionState::Disconnected);
        assert!(!status.is_connected);
        assert_eq!(status.pending_count, 0);
    }
}
