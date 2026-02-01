//! # Store Hub Server Module
//!
//! Implements the WebSocket server that runs when a device is elected PRIMARY.
//! This hub accepts connections from SECONDARY devices and coordinates inventory sync.
//!
//! ## Architecture
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                        Store Hub Architecture                           │
//! │                                                                         │
//! │  ┌─────────────────────────────────────────────────────────────────┐   │
//! │  │                      HubServer (Axum)                           │   │
//! │  │                                                                 │   │
//! │  │  /ws endpoint ──▶ WebSocket upgrade                            │   │
//! │  │                        │                                        │   │
//! │  │                        ▼                                        │   │
//! │  │              ┌─────────────────┐                                │   │
//! │  │              │  HubConnection  │ ◀───── Per-device              │   │
//! │  │              │    Handler      │        connection              │   │
//! │  │              └────────┬────────┘                                │   │
//! │  │                       │                                         │   │
//! │  │         ┌─────────────┼─────────────┐                          │   │
//! │  │         ▼             ▼             ▼                          │   │
//! │  │  ┌──────────┐  ┌──────────┐  ┌──────────┐                      │   │
//! │  │  │ POS #1   │  │ POS #2   │  │ POS #3   │   Connected          │   │
//! │  │  │(device_1)│  │(device_2)│  │(device_3)│   SECONDARY devices  │   │
//! │  │  └──────────┘  └──────────┘  └──────────┘                      │   │
//! │  │                                                                 │   │
//! │  └─────────────────────────────────────────────────────────────────┘   │
//! │                                                                         │
//! │  Message Flow:                                                          │
//! │  ─────────────                                                          │
//! │  1. SECONDARY connects with Hello message                              │
//! │  2. Hub responds with Welcome (includes current term)                  │
//! │  3. SECONDARY sends InventoryDelta messages                            │
//! │  4. Hub broadcasts InventoryUpdate to all connected devices            │
//! │  5. Hub sends periodic Heartbeat to maintain connection                │
//! │                                                                         │
//! └─────────────────────────────────────────────────────────────────────────┘
//! ```

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::IntoResponse,
    routing::get,
    Router,
};
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio::sync::{broadcast, mpsc, RwLock};
use tokio::time::{interval, Duration};
use tracing::{debug, error, info, warn};

use crate::config::SyncConfig;
use crate::election::ElectionHandle;
use crate::error::{SyncError, SyncResult};
use crate::protocol::{HelloPayload, SyncMessage, WelcomePayload};

// =============================================================================
// Constants
// =============================================================================

/// Default WebSocket port for the hub server.
pub const DEFAULT_HUB_PORT: u16 = 8765;

/// Ping interval to keep connections alive.
const PING_INTERVAL: Duration = Duration::from_secs(30);

/// Maximum message size (1MB).
const MAX_MESSAGE_SIZE: usize = 1024 * 1024;

// =============================================================================
// Hub Configuration
// =============================================================================

/// Configuration for the hub server.
#[derive(Debug, Clone)]
pub struct HubConfig {
    /// Port to listen on.
    pub port: u16,
    /// Bind address (default: 0.0.0.0).
    pub bind_addr: String,
}

impl Default for HubConfig {
    fn default() -> Self {
        HubConfig {
            port: DEFAULT_HUB_PORT,
            bind_addr: "0.0.0.0".to_string(),
        }
    }
}

impl HubConfig {
    /// Returns the full bind address.
    pub fn bind_address(&self) -> String {
        format!("{}:{}", self.bind_addr, self.port)
    }
}

// =============================================================================
// Connected Client
// =============================================================================

/// Represents a connected SECONDARY device.
#[derive(Debug, Clone)]
pub struct ConnectedClient {
    /// Device ID.
    pub device_id: String,
    /// Store ID.
    pub store_id: String,
    /// Client address.
    pub addr: SocketAddr,
    /// Connection time.
    pub connected_at: std::time::Instant,
}

// =============================================================================
// Hub State
// =============================================================================

/// Shared state for the hub server.
pub struct HubState {
    /// Sync configuration.
    sync_config: Arc<SyncConfig>,
    /// Election handle.
    election: ElectionHandle,
    /// Connected clients.
    clients: RwLock<HashMap<String, ConnectedClient>>,
    /// Broadcast channel for sending messages to all clients.
    broadcast_tx: broadcast::Sender<SyncMessage>,
    /// Channel for receiving inventory deltas from clients.
    delta_tx: mpsc::Sender<(String, SyncMessage)>,
}

impl HubState {
    /// Creates new hub state.
    fn new(
        sync_config: Arc<SyncConfig>,
        election: ElectionHandle,
        delta_tx: mpsc::Sender<(String, SyncMessage)>,
    ) -> Self {
        let (broadcast_tx, _) = broadcast::channel(256);
        HubState {
            sync_config,
            election,
            clients: RwLock::new(HashMap::new()),
            broadcast_tx,
            delta_tx,
        }
    }

    /// Broadcasts a message to all connected clients.
    pub fn broadcast(&self, msg: SyncMessage) -> SyncResult<()> {
        let _ = self.broadcast_tx.send(msg);
        Ok(())
    }

    /// Returns the number of connected clients.
    pub async fn client_count(&self) -> usize {
        self.clients.read().await.len()
    }

    /// Returns a list of connected client IDs.
    pub async fn client_ids(&self) -> Vec<String> {
        self.clients.read().await.keys().cloned().collect()
    }
}

// =============================================================================
// Hub Server
// =============================================================================

/// The main hub server that manages WebSocket connections.
pub struct HubServer {
    /// Hub configuration.
    config: HubConfig,
    /// Shared hub state.
    state: Arc<HubState>,
}

/// Handle for controlling the hub server.
#[derive(Clone)]
pub struct HubHandle {
    /// Shared state.
    state: Arc<HubState>,
    /// Shutdown signal sender.
    shutdown_tx: mpsc::Sender<()>,
}

impl HubHandle {
    /// Broadcasts a message to all connected clients.
    pub fn broadcast(&self, msg: SyncMessage) -> SyncResult<()> {
        self.state.broadcast(msg)
    }

    /// Returns the number of connected clients.
    pub async fn client_count(&self) -> usize {
        self.state.client_count().await
    }

    /// Returns a list of connected client IDs.
    pub async fn client_ids(&self) -> Vec<String> {
        self.state.client_ids().await
    }

    /// Shuts down the hub server.
    pub async fn shutdown(&self) -> SyncResult<()> {
        self.shutdown_tx
            .send(())
            .await
            .map_err(|_| SyncError::ChannelError("Hub shutdown channel closed".into()))
    }
}

impl HubServer {
    /// Creates a new hub server.
    pub fn new(
        config: HubConfig,
        sync_config: Arc<SyncConfig>,
        election: ElectionHandle,
        delta_tx: mpsc::Sender<(String, SyncMessage)>,
    ) -> Self {
        let state = Arc::new(HubState::new(sync_config, election, delta_tx));
        HubServer { config, state }
    }

    /// Starts the hub server and returns a handle.
    pub async fn start(self) -> SyncResult<HubHandle> {
        let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);

        let handle = HubHandle {
            state: self.state.clone(),
            shutdown_tx,
        };

        // Build the router
        let app = Router::new()
            .route("/ws", get(ws_handler))
            .route("/health", get(health_handler))
            .with_state(self.state.clone());

        // Bind the listener
        let bind_addr = self.config.bind_address();
        let listener = TcpListener::bind(&bind_addr)
            .await
            .map_err(|e| SyncError::TransportError(format!("Failed to bind to {}: {}", bind_addr, e)))?;

        info!(addr = %bind_addr, "Hub server started");

        // Spawn the server
        tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async move {
                    shutdown_rx.recv().await;
                    info!("Hub server shutting down");
                })
                .await
                .ok();
        });

        Ok(handle)
    }
}

// =============================================================================
// WebSocket Handler
// =============================================================================

/// Health check endpoint.
async fn health_handler() -> impl IntoResponse {
    "OK"
}

/// WebSocket upgrade handler.
async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<HubState>>,
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<SocketAddr>,
) -> impl IntoResponse {
    info!(addr = %addr, "New WebSocket connection");
    ws.max_message_size(MAX_MESSAGE_SIZE)
        .on_upgrade(move |socket| handle_socket(socket, state, addr))
}

/// Handles a WebSocket connection.
async fn handle_socket(socket: WebSocket, state: Arc<HubState>, addr: SocketAddr) {
    let (mut sender, mut receiver) = socket.split();

    // Wait for Hello message
    let hello = match receive_hello(&mut receiver).await {
        Ok(hello) => hello,
        Err(e) => {
            warn!(addr = %addr, ?e, "Failed to receive Hello - closing connection");
            return;
        }
    };

    let device_id = hello.device_id.clone();
    let store_id = hello.store_id.clone();

    // Verify store_id matches
    if store_id != state.sync_config.store_id() {
        warn!(
            device_id = %device_id,
            client_store = %store_id,
            our_store = %state.sync_config.store_id(),
            "Store ID mismatch - rejecting connection"
        );
        let reject_msg = SyncMessage::Error {
            code: "STORE_MISMATCH".to_string(),
            message: "Store ID does not match".to_string(),
        };
        if let Ok(json) = serde_json::to_string(&reject_msg) {
            let _ = sender.send(Message::Text(json.into())).await;
        }
        return;
    }

    info!(
        device_id = %device_id,
        store_id = %store_id,
        addr = %addr,
        "Client authenticated"
    );

    // Register client
    {
        let mut clients = state.clients.write().await;
        clients.insert(
            device_id.clone(),
            ConnectedClient {
                device_id: device_id.clone(),
                store_id: store_id.clone(),
                addr,
                connected_at: std::time::Instant::now(),
            },
        );
    }

    // Send Welcome message
    let term = state.election.term().await;
    let welcome = SyncMessage::Welcome(WelcomePayload {
        hub_device_id: state.sync_config.device_id().to_string(),
        store_id: state.sync_config.store_id().to_string(),
        election_term: term,
        server_time: chrono::Utc::now().to_rfc3339(),
    });

    if let Err(e) = send_message(&mut sender, &welcome).await {
        warn!(device_id = %device_id, ?e, "Failed to send Welcome");
        remove_client(&state, &device_id).await;
        return;
    }

    // Subscribe to broadcasts
    let mut broadcast_rx = state.broadcast_tx.subscribe();

    // Spawn task for sending broadcasts
    let sender_device_id = device_id.clone();
    let (outgoing_tx, mut outgoing_rx) = mpsc::channel::<Message>(64);

    // Outgoing message task
    let outgoing_handle = tokio::spawn(async move {
        while let Some(msg) = outgoing_rx.recv().await {
            if sender.send(msg).await.is_err() {
                break;
            }
        }
    });

    // Broadcast forwarding task
    let outgoing_tx_clone = outgoing_tx.clone();
    let broadcast_handle = tokio::spawn(async move {
        loop {
            match broadcast_rx.recv().await {
                Ok(msg) => {
                    // Don't send message back to originator
                    if let Ok(json) = serde_json::to_string(&msg) {
                        if outgoing_tx_clone.send(Message::Text(json.into())).await.is_err() {
                            break;
                        }
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    warn!(device_id = %sender_device_id, "Broadcast receiver lagged");
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    // Ping task
    let outgoing_tx_ping = outgoing_tx.clone();
    let ping_handle = tokio::spawn(async move {
        let mut ping_interval = interval(PING_INTERVAL);
        loop {
            ping_interval.tick().await;
            if outgoing_tx_ping.send(Message::Ping(axum::body::Bytes::new())).await.is_err() {
                break;
            }
        }
    });

    // Main receive loop
    loop {
        match receiver.next().await {
            Some(Ok(msg)) => {
                match msg {
                    Message::Text(text) => {
                        match serde_json::from_str::<SyncMessage>(&text) {
                            Ok(sync_msg) => {
                                handle_client_message(&state, &device_id, sync_msg).await;
                            }
                            Err(e) => {
                                debug!(device_id = %device_id, ?e, "Invalid message format");
                            }
                        }
                    }
                    Message::Binary(data) => {
                        match serde_json::from_slice::<SyncMessage>(&data) {
                            Ok(sync_msg) => {
                                handle_client_message(&state, &device_id, sync_msg).await;
                            }
                            Err(e) => {
                                debug!(device_id = %device_id, ?e, "Invalid binary message");
                            }
                        }
                    }
                    Message::Pong(_) => {
                        // Connection is alive
                    }
                    Message::Ping(data) => {
                        // Respond with pong
                        let _ = outgoing_tx.send(Message::Pong(data)).await;
                    }
                    Message::Close(_) => {
                        info!(device_id = %device_id, "Client requested close");
                        break;
                    }
                }
            }
            Some(Err(e)) => {
                warn!(device_id = %device_id, ?e, "WebSocket error");
                break;
            }
            None => {
                info!(device_id = %device_id, "Client disconnected");
                break;
            }
        }
    }

    // Cleanup
    ping_handle.abort();
    broadcast_handle.abort();
    outgoing_handle.abort();
    remove_client(&state, &device_id).await;
}

/// Receives and parses the Hello message.
async fn receive_hello(
    receiver: &mut futures_util::stream::SplitStream<WebSocket>,
) -> SyncResult<HelloPayload> {
    // Wait up to 10 seconds for Hello
    let timeout = tokio::time::timeout(Duration::from_secs(10), receiver.next()).await;

    match timeout {
        Ok(Some(Ok(msg))) => {
            let text = match msg {
                Message::Text(t) => t.to_string(),
                Message::Binary(b) => String::from_utf8_lossy(&b).to_string(),
                _ => return Err(SyncError::ProtocolError("Expected text message".into())),
            };

            let sync_msg: SyncMessage = serde_json::from_str(&text)
                .map_err(|e| SyncError::ProtocolError(format!("Invalid JSON: {}", e)))?;

            match sync_msg {
                SyncMessage::Hello(payload) => Ok(payload),
                _ => Err(SyncError::ProtocolError("Expected Hello message".into())),
            }
        }
        Ok(Some(Err(e))) => Err(SyncError::TransportError(format!("WebSocket error: {}", e))),
        Ok(None) => Err(SyncError::TransportError("Connection closed".into())),
        Err(_) => Err(SyncError::TransportError("Hello timeout".into())),
    }
}

/// Sends a SyncMessage.
async fn send_message(
    sender: &mut futures_util::stream::SplitSink<WebSocket, Message>,
    msg: &SyncMessage,
) -> SyncResult<()> {
    let json = serde_json::to_string(msg)
        .map_err(|e| SyncError::ProtocolError(format!("Serialization error: {}", e)))?;
    sender
        .send(Message::Text(json.into()))
        .await
        .map_err(|e| SyncError::TransportError(format!("Send error: {}", e)))?;
    Ok(())
}

/// Handles a message from a client.
async fn handle_client_message(state: &HubState, device_id: &str, msg: SyncMessage) {
    debug!(device_id = %device_id, ?msg, "Received client message");

    // Forward to delta processor
    if let Err(e) = state.delta_tx.send((device_id.to_string(), msg)).await {
        error!(?e, "Failed to forward message to delta processor");
    }
}

/// Removes a client from the connected list.
async fn remove_client(state: &HubState, device_id: &str) {
    let mut clients = state.clients.write().await;
    if clients.remove(device_id).is_some() {
        info!(device_id = %device_id, "Client removed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hub_config_default() {
        let config = HubConfig::default();
        assert_eq!(config.port, DEFAULT_HUB_PORT);
        assert_eq!(config.bind_addr, "0.0.0.0");
    }

    #[test]
    fn test_hub_config_bind_address() {
        let config = HubConfig {
            port: 9000,
            bind_addr: "127.0.0.1".to_string(),
        };
        assert_eq!(config.bind_address(), "127.0.0.1:9000");
    }
}
