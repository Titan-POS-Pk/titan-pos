//! # WebSocket Transport
//!
//! WebSocket client with automatic reconnection and backoff.
//!
//! ## Connection Lifecycle
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                    WebSocket Connection States                          │
//! │                                                                         │
//! │  ┌────────────┐    connect()    ┌────────────┐                         │
//! │  │Disconnected│ ──────────────► │ Connecting │                         │
//! │  └────────────┘                 └─────┬──────┘                         │
//! │        ▲                              │                                 │
//! │        │                    success   │   failure                       │
//! │        │                        ┌─────┴─────┐                          │
//! │        │                        ▼           ▼                           │
//! │        │              ┌────────────┐  ┌────────────┐                   │
//! │        │              │ Connected  │  │ Backoff    │                   │
//! │        │              └─────┬──────┘  └─────┬──────┘                   │
//! │        │                    │               │                           │
//! │        │              disconnect/error      │  timer expired            │
//! │        │                    │               │                           │
//! │        │                    ▼               │                           │
//! │        │              ┌────────────┐        │                           │
//! │        └───────────── │Reconnecting│ ◄──────┘                          │
//! │                       └────────────┘                                    │
//! │                                                                         │
//! │  BACKOFF STRATEGY (Exponential with Jitter)                            │
//! │  ───────────────────────────────────────────                           │
//! │  Attempt 1: 500ms                                                       │
//! │  Attempt 2: 1s                                                          │
//! │  Attempt 3: 2s                                                          │
//! │  ...                                                                    │
//! │  Max: 60s                                                               │
//! │                                                                         │
//! └─────────────────────────────────────────────────────────────────────────┘
//! ```

use backoff::backoff::Backoff;
use backoff::ExponentialBackoff;
use futures_util::stream::SplitSink;
use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::sync::{mpsc, Mutex, RwLock};
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};
use tracing::{debug, error, info, warn};

use crate::error::{SyncError, SyncResult};
use crate::protocol::SyncMessage;

// =============================================================================
// Transport State
// =============================================================================

/// Connection state for the WebSocket transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    /// Not connected.
    Disconnected,
    /// Attempting to connect.
    Connecting,
    /// Connected and ready.
    Connected,
    /// Waiting before reconnection attempt.
    Backoff,
    /// Reconnection in progress.
    Reconnecting,
}

impl std::fmt::Display for ConnectionState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConnectionState::Disconnected => write!(f, "disconnected"),
            ConnectionState::Connecting => write!(f, "connecting"),
            ConnectionState::Connected => write!(f, "connected"),
            ConnectionState::Backoff => write!(f, "backoff"),
            ConnectionState::Reconnecting => write!(f, "reconnecting"),
        }
    }
}

// =============================================================================
// Transport Configuration
// =============================================================================

/// Configuration for the WebSocket transport.
#[derive(Debug, Clone)]
pub struct TransportConfig {
    /// WebSocket URL to connect to.
    pub url: String,

    /// Connection timeout.
    pub connect_timeout: Duration,

    /// Initial backoff duration.
    pub initial_backoff: Duration,

    /// Maximum backoff duration.
    pub max_backoff: Duration,

    /// Maximum reconnection attempts (0 = infinite).
    pub max_retries: u32,

    /// Ping interval for keepalive.
    pub ping_interval: Duration,

    /// Pong timeout (disconnect if no pong received).
    pub pong_timeout: Duration,
}

impl Default for TransportConfig {
    fn default() -> Self {
        TransportConfig {
            url: String::new(),
            connect_timeout: Duration::from_secs(10),
            initial_backoff: Duration::from_millis(500),
            max_backoff: Duration::from_secs(60),
            max_retries: 0, // Infinite
            ping_interval: Duration::from_secs(30),
            pong_timeout: Duration::from_secs(10),
        }
    }
}

// =============================================================================
// Transport Handle
// =============================================================================

/// Handle for interacting with the transport from other components.
#[derive(Clone)]
pub struct TransportHandle {
    /// Sender for outgoing messages.
    outgoing_tx: mpsc::Sender<SyncMessage>,

    /// Current connection state.
    state: Arc<RwLock<ConnectionState>>,

    /// Shutdown signal.
    shutdown_tx: mpsc::Sender<()>,
}

impl TransportHandle {
    /// Sends a message through the transport.
    pub async fn send(&self, message: SyncMessage) -> SyncResult<()> {
        self.outgoing_tx
            .send(message)
            .await
            .map_err(|_| SyncError::ChannelError("Failed to send message".into()))
    }

    /// Returns the current connection state.
    pub async fn state(&self) -> ConnectionState {
        *self.state.read().await
    }

    /// Returns true if currently connected.
    pub async fn is_connected(&self) -> bool {
        *self.state.read().await == ConnectionState::Connected
    }

    /// Triggers graceful shutdown.
    pub async fn shutdown(&self) -> SyncResult<()> {
        self.shutdown_tx
            .send(())
            .await
            .map_err(|_| SyncError::ChannelError("Failed to send shutdown signal".into()))
    }
}

// =============================================================================
// WebSocket Transport
// =============================================================================

/// WebSocket transport with automatic reconnection.
///
/// ## Usage
/// ```rust,ignore
/// let config = TransportConfig {
///     url: "ws://localhost:8080/sync".into(),
///     ..Default::default()
/// };
///
/// let (handle, incoming_rx) = Transport::spawn(config)?;
///
/// // Send messages
/// handle.send(make_hello(...)?).await?;
///
/// // Receive messages
/// while let Some(msg) = incoming_rx.recv().await {
///     println!("Received: {:?}", msg.kind);
/// }
/// ```
pub struct Transport {
    config: TransportConfig,
    state: Arc<RwLock<ConnectionState>>,
    outgoing_rx: mpsc::Receiver<SyncMessage>,
    incoming_tx: mpsc::Sender<SyncMessage>,
    shutdown_rx: mpsc::Receiver<()>,
}

impl Transport {
    /// Creates a new transport and spawns its background task.
    ///
    /// Returns a handle for sending messages and a receiver for incoming messages.
    pub fn spawn(config: TransportConfig) -> (TransportHandle, mpsc::Receiver<SyncMessage>) {
        let (outgoing_tx, outgoing_rx) = mpsc::channel::<SyncMessage>(100);
        let (incoming_tx, incoming_rx) = mpsc::channel::<SyncMessage>(100);
        let (shutdown_tx, shutdown_rx) = mpsc::channel::<()>(1);
        let state = Arc::new(RwLock::new(ConnectionState::Disconnected));

        let transport = Transport {
            config,
            state: state.clone(),
            outgoing_rx,
            incoming_tx,
            shutdown_rx,
        };

        // Spawn background task
        tokio::spawn(transport.run());

        let handle = TransportHandle {
            outgoing_tx,
            state,
            shutdown_tx,
        };

        (handle, incoming_rx)
    }

    /// Main transport loop.
    async fn run(mut self) {
        info!(url = %self.config.url, "Transport starting");

        let mut backoff = self.create_backoff();
        let mut retry_count = 0u32;

        loop {
            // Check for shutdown
            if self.shutdown_rx.try_recv().is_ok() {
                info!("Transport received shutdown signal");
                break;
            }

            // Try to connect
            *self.state.write().await = ConnectionState::Connecting;

            match self.connect_with_timeout().await {
                Ok(ws_stream) => {
                    info!("WebSocket connected");
                    *self.state.write().await = ConnectionState::Connected;

                    // Reset backoff on successful connection
                    backoff.reset();
                    retry_count = 0;

                    // Run the connection loop
                    if let Err(e) = self.connection_loop(ws_stream).await {
                        warn!(?e, "Connection loop ended");
                    }
                }
                Err(e) => {
                    error!(?e, "Failed to connect");
                }
            }

            // Connection lost or failed - enter backoff
            *self.state.write().await = ConnectionState::Backoff;

            // Check retry limit
            if self.config.max_retries > 0 {
                retry_count += 1;
                if retry_count >= self.config.max_retries {
                    error!(
                        max_retries = self.config.max_retries,
                        "Max reconnection attempts reached"
                    );
                    break;
                }
            }

            // Wait for backoff duration
            if let Some(duration) = backoff.next_backoff() {
                debug!(?duration, attempt = retry_count, "Waiting before reconnect");

                tokio::select! {
                    _ = tokio::time::sleep(duration) => {
                        *self.state.write().await = ConnectionState::Reconnecting;
                    }
                    _ = self.shutdown_rx.recv() => {
                        info!("Shutdown during backoff");
                        break;
                    }
                }
            } else {
                // Backoff exhausted (shouldn't happen with infinite backoff)
                error!("Backoff exhausted");
                break;
            }
        }

        *self.state.write().await = ConnectionState::Disconnected;
        info!("Transport stopped");
    }

    /// Connects with timeout.
    async fn connect_with_timeout(
        &self,
    ) -> SyncResult<WebSocketStream<MaybeTlsStream<TcpStream>>> {
        let connect_future = connect_async(&self.config.url);

        match timeout(self.config.connect_timeout, connect_future).await {
            Ok(Ok((ws_stream, response))) => {
                debug!(status = ?response.status(), "WebSocket handshake complete");
                Ok(ws_stream)
            }
            Ok(Err(e)) => Err(SyncError::from(e)),
            Err(_) => Err(SyncError::Timeout(self.config.connect_timeout.as_secs())),
        }
    }

    /// Main connection loop - handles sending and receiving.
    async fn connection_loop(
        &mut self,
        ws_stream: WebSocketStream<MaybeTlsStream<TcpStream>>,
    ) -> SyncResult<()> {
        let (write, mut read) = ws_stream.split();
        let write = Arc::new(Mutex::new(write));

        let mut ping_interval = tokio::time::interval(self.config.ping_interval);
        ping_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            tokio::select! {
                // Handle outgoing messages
                Some(msg) = self.outgoing_rx.recv() => {
                    let json = msg.to_json()?;
                    debug!(msg_type = %msg.type_name(), "Sending message");
                    let mut writer = write.lock().await;
                    writer.send(WsMessage::Text(json.into())).await?;
                }

                // Handle incoming messages
                Some(result) = read.next() => {
                    match result {
                        Ok(WsMessage::Text(text)) => {
                            match SyncMessage::from_json(&text) {
                                Ok(msg) => {
                                    debug!(msg_type = %msg.type_name(), "Received message");
                                    if self.incoming_tx.send(msg).await.is_err() {
                                        warn!("Incoming message receiver dropped");
                                        return Err(SyncError::ChannelError("Receiver dropped".into()));
                                    }
                                }
                                Err(e) => {
                                    warn!(?e, "Failed to parse message");
                                }
                            }
                        }
                        Ok(WsMessage::Ping(data)) => {
                            let mut writer = write.lock().await;
                            writer.send(WsMessage::Pong(data)).await?;
                        }
                        Ok(WsMessage::Pong(_)) => {
                            debug!("Received pong");
                        }
                        Ok(WsMessage::Close(frame)) => {
                            info!(?frame, "Received close frame");
                            return Ok(());
                        }
                        Ok(WsMessage::Binary(_)) => {
                            warn!("Received unexpected binary message");
                        }
                        Ok(WsMessage::Frame(_)) => {
                            // Raw frame, ignore
                        }
                        Err(e) => {
                            error!(?e, "WebSocket error");
                            return Err(SyncError::from(e));
                        }
                    }
                }

                // Send periodic pings
                _ = ping_interval.tick() => {
                    let mut writer = write.lock().await;
                    writer.send(WsMessage::Ping(vec![].into())).await?;
                    debug!("Sent ping");
                }

                // Check for shutdown
                _ = self.shutdown_rx.recv() => {
                    info!("Shutdown signal received, closing connection");
                    let mut writer = write.lock().await;
                    let _ = writer.send(WsMessage::Close(None)).await;
                    return Ok(());
                }
            }
        }
    }

    /// Creates the exponential backoff configuration.
    fn create_backoff(&self) -> ExponentialBackoff {
        ExponentialBackoff {
            initial_interval: self.config.initial_backoff,
            max_interval: self.config.max_backoff,
            multiplier: 2.0,
            max_elapsed_time: None, // No limit on total time
            ..Default::default()
        }
    }
}

// =============================================================================
// Sender Wrapper (for use in other components)
// =============================================================================

/// Type alias for the WebSocket write half.
pub type WsSink = SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, WsMessage>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connection_state_display() {
        assert_eq!(ConnectionState::Connected.to_string(), "connected");
        assert_eq!(ConnectionState::Backoff.to_string(), "backoff");
    }

    #[test]
    fn test_transport_config_default() {
        let config = TransportConfig::default();
        assert_eq!(config.connect_timeout, Duration::from_secs(10));
        assert_eq!(config.max_retries, 0); // Infinite
    }
}
