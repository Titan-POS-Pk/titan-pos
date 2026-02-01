//! # Leader Election Module
//!
//! Implements leader election for automatic Store Hub selection.
//!
//! ## Election Protocol
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                    Leader Election Protocol                             │
//! │                                                                         │
//! │  ELECTION TRIGGER CONDITIONS:                                          │
//! │  ────────────────────────────                                          │
//! │  1. Device starts in AUTO mode and no PRIMARY found                    │
//! │  2. Current PRIMARY heartbeat timeout (15 seconds)                     │
//! │  3. Current PRIMARY explicitly resigns                                 │
//! │                                                                         │
//! │  ELECTION ALGORITHM:                                                   │
//! │  ───────────────────                                                   │
//! │  1. Candidate announces candidacy with (priority, device_id, term)     │
//! │  2. Wait for election_timeout (randomized: 150-300ms)                  │
//! │  3. If no higher-priority candidate seen, become PRIMARY               │
//! │  4. Broadcast hub announcement with new term                           │
//! │                                                                         │
//! │  PRIORITY COMPARISON:                                                  │
//! │  ────────────────────                                                  │
//! │  if candidate_a.priority > candidate_b.priority:                       │
//! │      candidate_a wins                                                   │
//! │  elif candidate_a.priority == candidate_b.priority:                    │
//! │      lexicographically_smaller(device_id) wins  // Deterministic       │
//! │                                                                         │
//! │  FENCING TOKEN:                                                        │
//! │  ──────────────                                                        │
//! │  • election_term increments on each election                           │
//! │  • SECONDARY rejects commands from PRIMARY with lower term             │
//! │  • Prevents split-brain scenarios                                      │
//! │                                                                         │
//! │  STATE TRANSITIONS:                                                    │
//! │  ───────────────────                                                   │
//! │                                                                         │
//! │  ┌────────────┐     no PRIMARY found     ┌─────────────┐               │
//! │  │  SECONDARY │ ──────────────────────▶  │  CANDIDATE  │               │
//! │  └─────┬──────┘                          └──────┬──────┘               │
//! │        │                                        │                       │
//! │        │  PRIMARY found                         │  win election         │
//! │        │  or lost election                      │                       │
//! │        │                                        ▼                       │
//! │        │                                 ┌─────────────┐                │
//! │        └────────────────────────────────▶│   PRIMARY   │                │
//! │                                          └──────┬──────┘                │
//! │                                                 │                       │
//! │              heartbeat timeout or resign        │                       │
//! │                                                 ▼                       │
//! │                                          ┌─────────────┐                │
//! │                                          │  SECONDARY  │                │
//! │                                          └─────────────┘                │
//! └─────────────────────────────────────────────────────────────────────────┘
//! ```

use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::{mpsc, watch, RwLock};
use tokio::time::{interval, sleep};
use tracing::{debug, info, warn};

use crate::config::{SyncConfig, SyncMode};
use crate::discovery::{discover_hubs, DiscoveredHub, DiscoveryConfig};
use crate::error::{SyncError, SyncResult};

// =============================================================================
// Constants
// =============================================================================

/// Default heartbeat interval (how often PRIMARY announces itself).
pub const DEFAULT_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);

/// Default heartbeat timeout (how long before PRIMARY is considered dead).
pub const DEFAULT_HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(15);

/// Minimum election timeout for randomization.
const MIN_ELECTION_TIMEOUT_MS: u64 = 150;

/// Maximum election timeout for randomization.
const MAX_ELECTION_TIMEOUT_MS: u64 = 300;

// =============================================================================
// Node Role
// =============================================================================

/// Current role of this node in the cluster.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeRole {
    /// This node is the store hub (PRIMARY).
    Primary,
    /// This node is connected to a hub (SECONDARY).
    Secondary,
    /// This node is campaigning to become PRIMARY.
    Candidate,
    /// This node is offline (no sync).
    Offline,
}

impl std::fmt::Display for NodeRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NodeRole::Primary => write!(f, "primary"),
            NodeRole::Secondary => write!(f, "secondary"),
            NodeRole::Candidate => write!(f, "candidate"),
            NodeRole::Offline => write!(f, "offline"),
        }
    }
}

// =============================================================================
// Election State
// =============================================================================

/// Current election state.
#[derive(Debug, Clone)]
pub struct ElectionState {
    /// Current role.
    pub role: NodeRole,
    /// Current election term (fencing token).
    pub term: u64,
    /// Known PRIMARY device ID (if any).
    pub primary_id: Option<String>,
    /// Known PRIMARY address (if any).
    pub primary_url: Option<String>,
    /// Last heartbeat from PRIMARY.
    pub last_heartbeat: Option<Instant>,
}

impl Default for ElectionState {
    fn default() -> Self {
        ElectionState {
            role: NodeRole::Secondary,
            term: 0,
            primary_id: None,
            primary_url: None,
            last_heartbeat: None,
        }
    }
}

// =============================================================================
// Election Configuration
// =============================================================================

/// Configuration for the election service.
#[derive(Debug, Clone)]
pub struct ElectionConfig {
    /// Heartbeat interval for PRIMARY.
    pub heartbeat_interval: Duration,
    /// Heartbeat timeout before triggering election.
    pub heartbeat_timeout: Duration,
    /// Discovery configuration.
    pub discovery_config: DiscoveryConfig,
}

impl Default for ElectionConfig {
    fn default() -> Self {
        ElectionConfig {
            heartbeat_interval: DEFAULT_HEARTBEAT_INTERVAL,
            heartbeat_timeout: DEFAULT_HEARTBEAT_TIMEOUT,
            discovery_config: DiscoveryConfig::default(),
        }
    }
}

// =============================================================================
// Election Service
// =============================================================================

/// Manages leader election and role transitions.
pub struct ElectionService {
    /// Sync configuration.
    sync_config: Arc<SyncConfig>,
    /// Election configuration.
    config: ElectionConfig,
    /// Current election state.
    state: Arc<RwLock<ElectionState>>,
    /// State change broadcaster.
    state_tx: watch::Sender<ElectionState>,
}

/// Handle for interacting with the election service.
#[derive(Clone)]
pub struct ElectionHandle {
    /// Current state.
    state: Arc<RwLock<ElectionState>>,
    /// State change receiver.
    state_rx: watch::Receiver<ElectionState>,
    /// Command sender.
    cmd_tx: mpsc::Sender<ElectionCommand>,
}

/// Commands that can be sent to the election service.
#[derive(Debug)]
pub enum ElectionCommand {
    /// Force this node to become PRIMARY (if allowed by config).
    ForcePrimary,
    /// Force this node to become SECONDARY.
    ForceSecondary,
    /// Trigger a new election.
    TriggerElection,
    /// Record a heartbeat from PRIMARY.
    RecordHeartbeat { device_id: String, term: u64, url: String },
    /// Shutdown the election service.
    Shutdown,
}

impl ElectionHandle {
    /// Returns the current election state.
    pub async fn state(&self) -> ElectionState {
        self.state.read().await.clone()
    }

    /// Returns the current role.
    pub async fn role(&self) -> NodeRole {
        self.state.read().await.role
    }

    /// Returns true if this node is PRIMARY.
    pub async fn is_primary(&self) -> bool {
        self.state.read().await.role == NodeRole::Primary
    }

    /// Returns the current term.
    pub async fn term(&self) -> u64 {
        self.state.read().await.term
    }

    /// Waits for a role change.
    pub async fn wait_for_role_change(&mut self) -> ElectionState {
        self.state_rx.changed().await.ok();
        self.state_rx.borrow().clone()
    }

    /// Subscribes to state changes.
    pub fn subscribe(&self) -> watch::Receiver<ElectionState> {
        self.state_rx.clone()
    }

    /// Records a heartbeat from the PRIMARY.
    pub async fn record_heartbeat(&self, device_id: String, term: u64, url: String) -> SyncResult<()> {
        self.cmd_tx
            .send(ElectionCommand::RecordHeartbeat { device_id, term, url })
            .await
            .map_err(|_| SyncError::ChannelError("Election command channel closed".into()))
    }

    /// Forces this node to become PRIMARY.
    pub async fn force_primary(&self) -> SyncResult<()> {
        self.cmd_tx
            .send(ElectionCommand::ForcePrimary)
            .await
            .map_err(|_| SyncError::ChannelError("Election command channel closed".into()))
    }

    /// Forces this node to become SECONDARY.
    pub async fn force_secondary(&self) -> SyncResult<()> {
        self.cmd_tx
            .send(ElectionCommand::ForceSecondary)
            .await
            .map_err(|_| SyncError::ChannelError("Election command channel closed".into()))
    }

    /// Triggers a new election.
    pub async fn trigger_election(&self) -> SyncResult<()> {
        self.cmd_tx
            .send(ElectionCommand::TriggerElection)
            .await
            .map_err(|_| SyncError::ChannelError("Election command channel closed".into()))
    }

    /// Shuts down the election service.
    pub async fn shutdown(&self) -> SyncResult<()> {
        self.cmd_tx
            .send(ElectionCommand::Shutdown)
            .await
            .map_err(|_| SyncError::ChannelError("Election command channel closed".into()))
    }
}

impl ElectionService {
    /// Creates a new election service.
    pub fn new(sync_config: Arc<SyncConfig>, config: ElectionConfig) -> Self {
        let initial_state = ElectionState::default();
        let (state_tx, _) = watch::channel(initial_state.clone());

        ElectionService {
            sync_config,
            config,
            state: Arc::new(RwLock::new(initial_state)),
            state_tx,
        }
    }

    /// Starts the election service and returns a handle.
    pub fn start(self) -> ElectionHandle {
        let (cmd_tx, cmd_rx) = mpsc::channel(32);
        let state_rx = self.state_tx.subscribe();

        let handle = ElectionHandle {
            state: self.state.clone(),
            state_rx,
            cmd_tx,
        };

        // Spawn the election loop
        tokio::spawn(async move {
            self.run(cmd_rx).await;
        });

        handle
    }

    /// Main election loop.
    async fn run(self, mut cmd_rx: mpsc::Receiver<ElectionCommand>) {
        info!(
            mode = %self.sync_config.mode(),
            device_id = %self.sync_config.device_id(),
            priority = self.sync_config.device.priority,
            "Election service started"
        );

        // Initialize based on sync mode
        let initial_role = match self.sync_config.mode() {
            SyncMode::Primary => {
                // Forced PRIMARY mode
                info!("Forced PRIMARY mode - becoming hub immediately");
                self.become_primary().await;
                NodeRole::Primary
            }
            SyncMode::Secondary => {
                // Forced SECONDARY mode
                info!("Forced SECONDARY mode - will not participate in elections");
                NodeRole::Secondary
            }
            SyncMode::Offline => {
                info!("Offline mode - election disabled");
                NodeRole::Offline
            }
            SyncMode::Auto => {
                // Start discovery and election
                info!("Auto mode - starting discovery");
                NodeRole::Secondary
            }
        };

        {
            let mut state = self.state.write().await;
            state.role = initial_role;
        }

        // If auto mode, do initial discovery
        if self.sync_config.mode() == SyncMode::Auto {
            self.do_discovery_and_election().await;
        }

        // Main loop: handle commands and heartbeat timeouts
        let mut heartbeat_check = interval(Duration::from_secs(1));

        loop {
            tokio::select! {
                Some(cmd) = cmd_rx.recv() => {
                    match cmd {
                        ElectionCommand::Shutdown => {
                            info!("Election service shutting down");
                            break;
                        }
                        ElectionCommand::ForcePrimary => {
                            if self.sync_config.mode().can_be_primary() {
                                self.become_primary().await;
                            } else {
                                warn!("Cannot force PRIMARY - mode doesn't allow it");
                            }
                        }
                        ElectionCommand::ForceSecondary => {
                            self.become_secondary(None).await;
                        }
                        ElectionCommand::TriggerElection => {
                            if self.sync_config.mode() == SyncMode::Auto {
                                self.do_discovery_and_election().await;
                            }
                        }
                        ElectionCommand::RecordHeartbeat { device_id, term, url } => {
                            self.handle_heartbeat(device_id, term, url).await;
                        }
                    }
                }
                _ = heartbeat_check.tick() => {
                    self.check_heartbeat_timeout().await;
                }
            }
        }
    }

    /// Performs discovery and potentially starts an election.
    async fn do_discovery_and_election(&self) {
        debug!("Running discovery scan");

        match discover_hubs(&self.config.discovery_config, &self.sync_config).await {
            Ok(hubs) => {
                if hubs.is_empty() {
                    info!("No hubs found - starting election");
                    self.run_election().await;
                } else {
                    // Find the best hub
                    let best_hub = hubs
                        .iter()
                        .max_by(|a, b| {
                            match a.priority.cmp(&b.priority) {
                                std::cmp::Ordering::Equal => b.device_id.cmp(&a.device_id),
                                other => other,
                            }
                        });

                    if let Some(hub) = best_hub {
                        info!(
                            hub_id = %hub.device_id,
                            hub_url = %hub.ws_url(),
                            "Found existing hub"
                        );

                        // Check if we should challenge (higher priority)
                        if self.should_challenge(hub) {
                            info!("We have higher priority - challenging current hub");
                            self.run_election().await;
                        } else {
                            self.become_secondary(Some(hub.clone())).await;
                        }
                    }
                }
            }
            Err(e) => {
                warn!(?e, "Discovery failed - assuming we're alone, becoming PRIMARY");
                self.run_election().await;
            }
        }
    }

    /// Checks if we should challenge the current hub.
    fn should_challenge(&self, hub: &DiscoveredHub) -> bool {
        let our_priority = self.sync_config.device.priority;
        let our_id = self.sync_config.device_id();

        if our_priority > hub.priority {
            return true;
        }
        if our_priority == hub.priority && our_id < hub.device_id.as_str() {
            return true;
        }
        false
    }

    /// Runs an election.
    async fn run_election(&self) {
        if !self.sync_config.mode().can_be_primary() {
            debug!("Cannot run election - mode doesn't allow PRIMARY");
            return;
        }

        // Become candidate and get new term
        let new_term = {
            let mut state = self.state.write().await;
            state.role = NodeRole::Candidate;
            state.term += 1;
            let term = state.term;
            let _ = self.state_tx.send(state.clone());
            term
        };

        info!(term = new_term, "Starting election as candidate");

        // Random election timeout to prevent split-brain
        let timeout_ms = MIN_ELECTION_TIMEOUT_MS
            + (rand_u64() % (MAX_ELECTION_TIMEOUT_MS - MIN_ELECTION_TIMEOUT_MS));
        sleep(Duration::from_millis(timeout_ms)).await;

        // Check if we're still a candidate (someone else might have won)
        let should_become_primary = {
            let state = self.state.read().await;
            state.role == NodeRole::Candidate
        };

        if should_become_primary {
            // No one else claimed PRIMARY - we win!
            self.become_primary().await;
        }
    }

    /// Transitions to PRIMARY role.
    async fn become_primary(&self) {
        let mut state = self.state.write().await;
        state.role = NodeRole::Primary;
        state.primary_id = Some(self.sync_config.device_id().to_string());
        state.primary_url = None; // We ARE the primary
        state.last_heartbeat = Some(Instant::now());

        info!(
            term = state.term,
            device_id = %self.sync_config.device_id(),
            "Became PRIMARY (Store Hub)"
        );

        let _ = self.state_tx.send(state.clone());
    }

    /// Transitions to SECONDARY role.
    async fn become_secondary(&self, hub: Option<DiscoveredHub>) {
        let mut state = self.state.write().await;
        state.role = NodeRole::Secondary;

        if let Some(hub) = hub {
            state.primary_id = Some(hub.device_id.clone());
            state.primary_url = Some(hub.ws_url());
            state.term = hub.election_term;
            state.last_heartbeat = Some(Instant::now());

            info!(
                primary_id = %hub.device_id,
                primary_url = %hub.ws_url(),
                term = hub.election_term,
                "Became SECONDARY - connected to hub"
            );
        } else {
            state.primary_id = None;
            state.primary_url = None;
            state.last_heartbeat = None;

            info!("Became SECONDARY - no hub connection");
        }

        let _ = self.state_tx.send(state.clone());
    }

    /// Handles a heartbeat from the PRIMARY.
    async fn handle_heartbeat(&self, device_id: String, term: u64, url: String) {
        let mut state = self.state.write().await;

        // Only accept heartbeats from higher or equal terms
        if term < state.term {
            debug!(
                received_term = term,
                our_term = state.term,
                "Ignoring stale heartbeat"
            );
            return;
        }

        // If we receive a heartbeat with higher term while PRIMARY, step down
        if state.role == NodeRole::Primary && term > state.term {
            warn!(
                new_term = term,
                old_term = state.term,
                new_primary = %device_id,
                "Higher term received - stepping down"
            );
            state.role = NodeRole::Secondary;
        }

        // Update state
        if term >= state.term {
            state.term = term;
            state.primary_id = Some(device_id);
            state.primary_url = Some(url);
            state.last_heartbeat = Some(Instant::now());

            if state.role == NodeRole::Candidate {
                // Someone else won the election
                debug!("Another node won the election");
                state.role = NodeRole::Secondary;
            }
        }

        let _ = self.state_tx.send(state.clone());
    }

    /// Checks if the PRIMARY heartbeat has timed out.
    async fn check_heartbeat_timeout(&self) {
        // Only check if we're SECONDARY
        let should_trigger_election = {
            let state = self.state.read().await;
            if state.role != NodeRole::Secondary {
                return;
            }

            if let Some(last_heartbeat) = state.last_heartbeat {
                last_heartbeat.elapsed() > self.config.heartbeat_timeout
            } else {
                // No heartbeat ever received - trigger discovery
                true
            }
        };

        if should_trigger_election && self.sync_config.mode() == SyncMode::Auto {
            warn!("PRIMARY heartbeat timeout - triggering election");
            self.do_discovery_and_election().await;
        }
    }
}

/// Simple random number generator (not cryptographically secure, just for jitter).
fn rand_u64() -> u64 {
    use std::time::SystemTime;
    let duration = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    // Mix in nanoseconds for some randomness
    duration.as_nanos() as u64 ^ (duration.as_secs() * 1_000_000_007)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_role_display() {
        assert_eq!(NodeRole::Primary.to_string(), "primary");
        assert_eq!(NodeRole::Secondary.to_string(), "secondary");
        assert_eq!(NodeRole::Candidate.to_string(), "candidate");
        assert_eq!(NodeRole::Offline.to_string(), "offline");
    }

    #[test]
    fn test_election_state_default() {
        let state = ElectionState::default();
        assert_eq!(state.role, NodeRole::Secondary);
        assert_eq!(state.term, 0);
        assert!(state.primary_id.is_none());
    }

    #[test]
    fn test_rand_u64_produces_different_values() {
        let a = rand_u64();
        std::thread::sleep(std::time::Duration::from_millis(1));
        let b = rand_u64();
        // They might be the same in rare cases, but generally should differ
        assert_ne!(a, b);
    }
}
