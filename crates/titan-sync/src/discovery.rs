//! # Discovery Module
//!
//! Implements device discovery for the Store Hub using mDNS and UDP broadcast.
//!
//! ## Discovery Protocol Flow
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                      Discovery Protocol                                  │
//! │                                                                          │
//! │  PRIMARY (Store Hub) Behavior:                                          │
//! │  ─────────────────────────────                                          │
//! │  1. Register mDNS service: _titan-pos._tcp.local                        │
//! │  2. Listen on UDP broadcast port 5555                                   │
//! │  3. Respond to discovery requests with hub address                      │
//! │                                                                          │
//! │  SECONDARY Behavior:                                                    │
//! │  ─────────────────                                                      │
//! │  1. Query mDNS for _titan-pos._tcp.local (primary method)               │
//! │  2. If mDNS fails, send UDP broadcast discovery request                 │
//! │  3. Wait for response with hub WebSocket URL                            │
//! │                                                                          │
//! │  ┌─────────────┐        mDNS Query         ┌─────────────┐              │
//! │  │  SECONDARY  │ ──────────────────────▶   │   PRIMARY   │              │
//! │  │             │        mDNS Response      │  (Store Hub)│              │
//! │  │             │ ◀──────────────────────   │             │              │
//! │  └─────────────┘                           └─────────────┘              │
//! │                                                                          │
//! │  Fallback (if mDNS unavailable):                                        │
//! │  ┌─────────────┐      UDP Broadcast        ┌─────────────┐              │
//! │  │  SECONDARY  │ ══════════════════════▶   │   PRIMARY   │              │
//! │  │             │      UDP Unicast Reply    │  (Store Hub)│              │
//! │  │             │ ◀──────────────────────   │             │              │
//! │  └─────────────┘                           └─────────────┘              │
//! └─────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Configuration
//! - mDNS service type: `_titan-pos._tcp.local`
//! - UDP discovery port: 5555 (configurable)
//! - Discovery timeout: 5 seconds

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::Arc;
use std::time::Duration;

use tokio::net::UdpSocket;
use tokio::sync::{mpsc, RwLock};
use tokio::time::{timeout, Instant};
use tracing::{debug, info, warn};

use crate::config::SyncConfig;
use crate::error::{SyncError, SyncResult};

// =============================================================================
// Constants
// =============================================================================

/// mDNS service type for Titan POS.
pub const MDNS_SERVICE_TYPE: &str = "_titan-pos._tcp.local.";

/// Default UDP discovery port.
pub const DEFAULT_DISCOVERY_PORT: u16 = 5555;

/// Discovery message magic bytes for validation.
const DISCOVERY_MAGIC: &[u8; 4] = b"TPOS";

/// Protocol version for discovery messages.
const DISCOVERY_PROTOCOL_VERSION: u8 = 1;

// =============================================================================
// Discovery Messages
// =============================================================================

/// Discovery message types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum DiscoveryMessageType {
    /// Request to find a hub.
    HubRequest = 1,
    /// Response announcing hub presence.
    HubAnnounce = 2,
    /// Heartbeat from hub.
    HubHeartbeat = 3,
}

impl TryFrom<u8> for DiscoveryMessageType {
    type Error = SyncError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(DiscoveryMessageType::HubRequest),
            2 => Ok(DiscoveryMessageType::HubAnnounce),
            3 => Ok(DiscoveryMessageType::HubHeartbeat),
            _ => Err(SyncError::InvalidMessage(format!(
                "Unknown discovery message type: {}",
                value
            ))),
        }
    }
}

/// Information about a discovered hub.
#[derive(Debug, Clone)]
pub struct DiscoveredHub {
    /// Device ID of the hub.
    pub device_id: String,
    /// Device name of the hub.
    pub device_name: String,
    /// Store ID the hub belongs to.
    pub store_id: String,
    /// IP address of the hub.
    pub ip_address: IpAddr,
    /// WebSocket port of the hub.
    pub ws_port: u16,
    /// Election term of the hub.
    pub election_term: u64,
    /// Priority of the hub.
    pub priority: u8,
    /// When this hub was discovered.
    pub discovered_at: Instant,
}

impl DiscoveredHub {
    /// Returns the WebSocket URL for connecting to this hub.
    pub fn ws_url(&self) -> String {
        format!("ws://{}:{}/sync", self.ip_address, self.ws_port)
    }
}

// =============================================================================
// Discovery Configuration
// =============================================================================

/// Configuration for the discovery service.
#[derive(Debug, Clone)]
pub struct DiscoveryConfig {
    /// UDP port for discovery broadcasts.
    pub discovery_port: u16,
    /// WebSocket port to announce (when acting as hub).
    pub ws_port: u16,
    /// Discovery timeout duration.
    pub discovery_timeout: Duration,
    /// Interval between hub announcements (when acting as hub).
    pub announce_interval: Duration,
    /// Whether mDNS is enabled.
    pub mdns_enabled: bool,
    /// Whether UDP discovery is enabled.
    pub udp_enabled: bool,
}

impl Default for DiscoveryConfig {
    fn default() -> Self {
        DiscoveryConfig {
            discovery_port: DEFAULT_DISCOVERY_PORT,
            ws_port: 8765,
            discovery_timeout: Duration::from_secs(5),
            announce_interval: Duration::from_secs(5),
            mdns_enabled: true,
            udp_enabled: true,
        }
    }
}

// =============================================================================
// Discovery Service
// =============================================================================

/// Discovery service for finding and announcing store hubs.
pub struct DiscoveryService {
    /// Configuration.
    config: DiscoveryConfig,
    /// Sync configuration (for device/store info).
    sync_config: Arc<SyncConfig>,
    /// Known hubs (device_id -> DiscoveredHub).
    known_hubs: Arc<RwLock<HashMap<String, DiscoveredHub>>>,
    /// UDP socket for discovery.
    socket: Option<Arc<UdpSocket>>,
    /// Shutdown sender.
    shutdown_tx: Option<mpsc::Sender<()>>,
}

/// Handle for controlling the discovery service.
#[derive(Clone)]
pub struct DiscoveryHandle {
    /// Known hubs.
    known_hubs: Arc<RwLock<HashMap<String, DiscoveredHub>>>,
    /// Channel to trigger discovery.
    discover_tx: mpsc::Sender<()>,
    /// Shutdown sender.
    shutdown_tx: mpsc::Sender<()>,
}

impl DiscoveryHandle {
    /// Triggers a discovery scan.
    pub async fn trigger_discovery(&self) -> SyncResult<()> {
        self.discover_tx
            .send(())
            .await
            .map_err(|_| SyncError::ChannelError("Discovery channel closed".into()))
    }

    /// Returns all known hubs.
    pub async fn known_hubs(&self) -> Vec<DiscoveredHub> {
        self.known_hubs.read().await.values().cloned().collect()
    }

    /// Returns the best hub (highest priority, then by device_id for tiebreak).
    pub async fn best_hub(&self) -> Option<DiscoveredHub> {
        let hubs = self.known_hubs.read().await;
        hubs.values()
            .max_by(|a, b| {
                // Higher priority wins, then lower device_id (lexicographic) for determinism
                match a.priority.cmp(&b.priority) {
                    std::cmp::Ordering::Equal => b.device_id.cmp(&a.device_id),
                    other => other,
                }
            })
            .cloned()
    }

    /// Triggers graceful shutdown.
    pub async fn shutdown(&self) -> SyncResult<()> {
        self.shutdown_tx
            .send(())
            .await
            .map_err(|_| SyncError::ChannelError("Shutdown channel closed".into()))
    }
}

impl DiscoveryService {
    /// Creates a new discovery service.
    pub fn new(config: DiscoveryConfig, sync_config: Arc<SyncConfig>) -> Self {
        DiscoveryService {
            config,
            sync_config,
            known_hubs: Arc::new(RwLock::new(HashMap::new())),
            socket: None,
            shutdown_tx: None,
        }
    }

    /// Starts the discovery service and returns a handle.
    pub async fn start(mut self) -> SyncResult<DiscoveryHandle> {
        // Bind UDP socket for discovery
        let bind_addr = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, self.config.discovery_port);
        let socket = UdpSocket::bind(bind_addr).await.map_err(|e| {
            SyncError::ConnectionFailed(format!(
                "Failed to bind discovery socket on port {}: {}",
                self.config.discovery_port, e
            ))
        })?;

        // Enable broadcast
        socket.set_broadcast(true).map_err(|e| {
            SyncError::ConnectionFailed(format!("Failed to enable broadcast: {}", e))
        })?;

        info!(port = self.config.discovery_port, "Discovery service started");

        let socket = Arc::new(socket);
        self.socket = Some(socket.clone());

        let (shutdown_tx, shutdown_rx) = mpsc::channel(1);
        let (discover_tx, discover_rx) = mpsc::channel(10);

        self.shutdown_tx = Some(shutdown_tx.clone());

        let handle = DiscoveryHandle {
            known_hubs: self.known_hubs.clone(),
            discover_tx,
            shutdown_tx,
        };

        // Spawn the discovery listener task
        let listener_socket = socket.clone();
        let listener_hubs = self.known_hubs.clone();
        let listener_config = self.sync_config.clone();
        tokio::spawn(async move {
            Self::run_listener(listener_socket, listener_hubs, listener_config, shutdown_rx).await;
        });

        // Spawn the discovery requester task
        let requester_socket = socket;
        let requester_config = self.config.clone();
        let requester_sync_config = self.sync_config.clone();
        let requester_hubs = self.known_hubs.clone();
        tokio::spawn(async move {
            Self::run_requester(
                requester_socket,
                requester_config,
                requester_sync_config,
                requester_hubs,
                discover_rx,
            )
            .await;
        });

        Ok(handle)
    }

    /// Runs the UDP listener for discovery messages.
    async fn run_listener(
        socket: Arc<UdpSocket>,
        known_hubs: Arc<RwLock<HashMap<String, DiscoveredHub>>>,
        sync_config: Arc<SyncConfig>,
        mut shutdown_rx: mpsc::Receiver<()>,
    ) {
        let mut buf = [0u8; 1024];

        loop {
            tokio::select! {
                _ = shutdown_rx.recv() => {
                    info!("Discovery listener shutting down");
                    break;
                }
                result = socket.recv_from(&mut buf) => {
                    match result {
                        Ok((len, addr)) => {
                            if let Err(e) = Self::handle_message(
                                &buf[..len],
                                addr,
                                &socket,
                                &known_hubs,
                                &sync_config,
                            ).await {
                                debug!(?e, "Failed to handle discovery message");
                            }
                        }
                        Err(e) => {
                            warn!(?e, "Error receiving discovery message");
                        }
                    }
                }
            }
        }
    }

    /// Handles an incoming discovery message.
    async fn handle_message(
        data: &[u8],
        from: SocketAddr,
        _socket: &UdpSocket,
        known_hubs: &RwLock<HashMap<String, DiscoveredHub>>,
        sync_config: &SyncConfig,
    ) -> SyncResult<()> {
        // Validate magic bytes
        if data.len() < 6 || &data[0..4] != DISCOVERY_MAGIC {
            return Err(SyncError::InvalidMessage("Invalid discovery magic".into()));
        }

        let version = data[4];
        if version != DISCOVERY_PROTOCOL_VERSION {
            return Err(SyncError::InvalidMessage(format!(
                "Unsupported discovery protocol version: {}",
                version
            )));
        }

        let msg_type = DiscoveryMessageType::try_from(data[5])?;
        let payload = &data[6..];

        match msg_type {
            DiscoveryMessageType::HubRequest => {
                debug!(?from, "Received hub request");
                // Only respond if we're the hub (handled by hub server)
                // This is a no-op here; the hub server will respond
            }
            DiscoveryMessageType::HubAnnounce | DiscoveryMessageType::HubHeartbeat => {
                // Parse hub announcement
                if let Some(hub) = Self::parse_hub_announce(payload, from.ip())? {
                    // Don't add ourselves
                    if hub.device_id != sync_config.device_id() {
                        debug!(
                            device_id = %hub.device_id,
                            ip = %hub.ip_address,
                            port = hub.ws_port,
                            "Discovered hub"
                        );
                        known_hubs.write().await.insert(hub.device_id.clone(), hub);
                    }
                }
            }
        }

        Ok(())
    }

    /// Parses a hub announcement payload.
    fn parse_hub_announce(payload: &[u8], from_ip: IpAddr) -> SyncResult<Option<DiscoveredHub>> {
        // Payload format:
        // - 2 bytes: ws_port (big-endian)
        // - 8 bytes: election_term (big-endian)
        // - 1 byte: priority
        // - 1 byte: device_id_len
        // - N bytes: device_id (UTF-8)
        // - 1 byte: device_name_len
        // - N bytes: device_name (UTF-8)
        // - 1 byte: store_id_len
        // - N bytes: store_id (UTF-8)

        if payload.len() < 13 {
            return Err(SyncError::InvalidMessage("Hub announce too short".into()));
        }

        let ws_port = u16::from_be_bytes([payload[0], payload[1]]);
        let election_term = u64::from_be_bytes([
            payload[2], payload[3], payload[4], payload[5], payload[6], payload[7], payload[8],
            payload[9],
        ]);
        let priority = payload[10];
        let device_id_len = payload[11] as usize;

        let mut offset = 12;
        if payload.len() < offset + device_id_len {
            return Err(SyncError::InvalidMessage("Device ID truncated".into()));
        }
        let device_id =
            String::from_utf8(payload[offset..offset + device_id_len].to_vec()).map_err(|_| {
                SyncError::InvalidMessage("Invalid device_id UTF-8".into())
            })?;
        offset += device_id_len;

        if payload.len() < offset + 1 {
            return Err(SyncError::InvalidMessage("Device name length missing".into()));
        }
        let device_name_len = payload[offset] as usize;
        offset += 1;

        if payload.len() < offset + device_name_len {
            return Err(SyncError::InvalidMessage("Device name truncated".into()));
        }
        let device_name = String::from_utf8(payload[offset..offset + device_name_len].to_vec())
            .map_err(|_| SyncError::InvalidMessage("Invalid device_name UTF-8".into()))?;
        offset += device_name_len;

        if payload.len() < offset + 1 {
            return Err(SyncError::InvalidMessage("Store ID length missing".into()));
        }
        let store_id_len = payload[offset] as usize;
        offset += 1;

        if payload.len() < offset + store_id_len {
            return Err(SyncError::InvalidMessage("Store ID truncated".into()));
        }
        let store_id = String::from_utf8(payload[offset..offset + store_id_len].to_vec())
            .map_err(|_| SyncError::InvalidMessage("Invalid store_id UTF-8".into()))?;

        Ok(Some(DiscoveredHub {
            device_id,
            device_name,
            store_id,
            ip_address: from_ip,
            ws_port,
            election_term,
            priority,
            discovered_at: Instant::now(),
        }))
    }

    /// Runs the discovery requester (sends broadcast requests).
    async fn run_requester(
        socket: Arc<UdpSocket>,
        config: DiscoveryConfig,
        sync_config: Arc<SyncConfig>,
        _known_hubs: Arc<RwLock<HashMap<String, DiscoveredHub>>>,
        mut discover_rx: mpsc::Receiver<()>,
    ) {
        loop {
            // Wait for a discovery trigger
            if discover_rx.recv().await.is_none() {
                info!("Discovery requester shutting down");
                break;
            }

            debug!("Sending discovery broadcast");

            // Build discovery request message
            let msg = Self::build_discovery_request(&sync_config);

            // Send broadcast to 255.255.255.255
            let broadcast_addr = SocketAddr::new(
                IpAddr::V4(Ipv4Addr::BROADCAST),
                config.discovery_port,
            );

            if let Err(e) = socket.send_to(&msg, broadcast_addr).await {
                warn!(?e, "Failed to send discovery broadcast");
            }
        }
    }

    /// Builds a discovery request message.
    fn build_discovery_request(sync_config: &SyncConfig) -> Vec<u8> {
        let mut msg = Vec::with_capacity(64);

        // Magic bytes
        msg.extend_from_slice(DISCOVERY_MAGIC);
        // Version
        msg.push(DISCOVERY_PROTOCOL_VERSION);
        // Message type
        msg.push(DiscoveryMessageType::HubRequest as u8);

        // Payload: store_id so hub knows we're from the same store
        let store_id = sync_config.store_id().as_bytes();
        msg.push(store_id.len() as u8);
        msg.extend_from_slice(store_id);

        msg
    }

    /// Builds a hub announcement message.
    pub fn build_hub_announce(
        sync_config: &SyncConfig,
        ws_port: u16,
        election_term: u64,
    ) -> Vec<u8> {
        let mut msg = Vec::with_capacity(256);

        // Magic bytes
        msg.extend_from_slice(DISCOVERY_MAGIC);
        // Version
        msg.push(DISCOVERY_PROTOCOL_VERSION);
        // Message type
        msg.push(DiscoveryMessageType::HubAnnounce as u8);

        // Payload
        msg.extend_from_slice(&ws_port.to_be_bytes());
        msg.extend_from_slice(&election_term.to_be_bytes());
        msg.push(sync_config.device.priority);

        let device_id = sync_config.device_id().as_bytes();
        msg.push(device_id.len() as u8);
        msg.extend_from_slice(device_id);

        let device_name = sync_config.device.name.as_bytes();
        msg.push(device_name.len() as u8);
        msg.extend_from_slice(device_name);

        let store_id = sync_config.store_id().as_bytes();
        msg.push(store_id.len() as u8);
        msg.extend_from_slice(store_id);

        msg
    }
}

/// Performs a one-shot discovery scan and returns discovered hubs.
///
/// ## Discovery Flow
/// ```text
/// ┌─────────────────────────────────────────────────────────────────────────┐
/// │  discover_hubs() - One-Shot Discovery                                   │
/// │                                                                         │
/// │  1. Bind temporary UDP socket                                          │
/// │  2. Send broadcast discovery request                                   │
/// │  3. Wait for responses (with timeout)                                  │
/// │  4. Return all discovered hubs                                         │
/// │                                                                         │
/// │  Timeline:                                                              │
/// │  ─────────────────────────────────────────────────────────────────────▶│
/// │  T+0ms     T+100ms    T+500ms    T+3000ms   T+5000ms                   │
/// │  │         │          │          │          │                          │
/// │  Send      Recv       Recv       Recv       Timeout                    │
/// │  Request   Response1  Response2  Response3  Return                     │
/// └─────────────────────────────────────────────────────────────────────────┘
/// ```
pub async fn discover_hubs(
    config: &DiscoveryConfig,
    sync_config: &SyncConfig,
) -> SyncResult<Vec<DiscoveredHub>> {
    info!("Starting hub discovery scan");

    // Bind to any available port for sending
    let socket = UdpSocket::bind("0.0.0.0:0").await.map_err(|e| {
        SyncError::ConnectionFailed(format!("Failed to bind discovery socket: {}", e))
    })?;

    socket.set_broadcast(true).map_err(|e| {
        SyncError::ConnectionFailed(format!("Failed to enable broadcast: {}", e))
    })?;

    // Build and send discovery request
    let request = DiscoveryService::build_discovery_request(sync_config);
    let broadcast_addr = SocketAddr::new(
        IpAddr::V4(Ipv4Addr::BROADCAST),
        config.discovery_port,
    );

    socket.send_to(&request, broadcast_addr).await.map_err(|e| {
        SyncError::ConnectionFailed(format!("Failed to send discovery broadcast: {}", e))
    })?;

    debug!("Sent discovery broadcast, waiting for responses");

    // Collect responses until timeout
    let mut hubs = HashMap::new();
    let mut buf = [0u8; 1024];

    let deadline = Instant::now() + config.discovery_timeout;

    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }

        match timeout(remaining, socket.recv_from(&mut buf)).await {
            Ok(Ok((len, addr))) => {
                let data = &buf[..len];

                // Validate and parse
                if data.len() >= 6
                    && &data[0..4] == DISCOVERY_MAGIC
                    && data[4] == DISCOVERY_PROTOCOL_VERSION
                {
                    let msg_type = data[5];
                    if msg_type == DiscoveryMessageType::HubAnnounce as u8
                        || msg_type == DiscoveryMessageType::HubHeartbeat as u8
                    {
                        if let Ok(Some(hub)) =
                            DiscoveryService::parse_hub_announce(&data[6..], addr.ip())
                        {
                            // Skip ourselves
                            if hub.device_id != sync_config.device_id() {
                                info!(
                                    device_id = %hub.device_id,
                                    ip = %hub.ip_address,
                                    ws_port = hub.ws_port,
                                    "Found hub"
                                );
                                hubs.insert(hub.device_id.clone(), hub);
                            }
                        }
                    }
                }
            }
            Ok(Err(e)) => {
                debug!(?e, "Error receiving discovery response");
            }
            Err(_) => {
                // Timeout
                break;
            }
        }
    }

    let result: Vec<DiscoveredHub> = hubs.into_values().collect();
    info!(count = result.len(), "Discovery scan complete");

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_discovery_message_type_roundtrip() {
        assert_eq!(
            DiscoveryMessageType::try_from(1).unwrap(),
            DiscoveryMessageType::HubRequest
        );
        assert_eq!(
            DiscoveryMessageType::try_from(2).unwrap(),
            DiscoveryMessageType::HubAnnounce
        );
        assert_eq!(
            DiscoveryMessageType::try_from(3).unwrap(),
            DiscoveryMessageType::HubHeartbeat
        );
        assert!(DiscoveryMessageType::try_from(99).is_err());
    }

    #[test]
    fn test_build_discovery_request() {
        let sync_config = SyncConfig::default();
        let msg = DiscoveryService::build_discovery_request(&sync_config);

        // Check magic
        assert_eq!(&msg[0..4], DISCOVERY_MAGIC);
        // Check version
        assert_eq!(msg[4], DISCOVERY_PROTOCOL_VERSION);
        // Check message type
        assert_eq!(msg[5], DiscoveryMessageType::HubRequest as u8);
    }

    #[test]
    fn test_build_hub_announce() {
        let sync_config = SyncConfig::default();
        let msg = DiscoveryService::build_hub_announce(&sync_config, 8765, 1);

        // Check magic
        assert_eq!(&msg[0..4], DISCOVERY_MAGIC);
        // Check version
        assert_eq!(msg[4], DISCOVERY_PROTOCOL_VERSION);
        // Check message type
        assert_eq!(msg[5], DiscoveryMessageType::HubAnnounce as u8);
        // Check ws_port
        assert_eq!(u16::from_be_bytes([msg[6], msg[7]]), 8765);
    }

    #[test]
    fn test_discovered_hub_ws_url() {
        let hub = DiscoveredHub {
            device_id: "test-device".into(),
            device_name: "Test".into(),
            store_id: "store-1".into(),
            ip_address: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100)),
            ws_port: 8765,
            election_term: 1,
            priority: 50,
            discovered_at: Instant::now(),
        };

        assert_eq!(hub.ws_url(), "ws://192.168.1.100:8765/sync");
    }
}
