//! # Standalone Hub Server
//!
//! A standalone WebSocket hub server that all POS terminals connect to.
//! Unlike the embedded hub in PRIMARY mode, this runs as a separate process.
//!
//! ## Usage
//! ```bash
//! # Start the hub server (local only)
//! cargo run -p titan-sync --bin hub-server -- --port 8765 --store-id demo-store
//!
//! # Start with cloud sync enabled
//! cargo run -p titan-sync --bin hub-server -- --port 8765 --store-id demo-store \
//!     --cloud-url http://localhost:50051 \
//!     --tenant-id demo-tenant \
//!     --api-key demo-api-key
//!
//! # All POS terminals connect as SECONDARY
//! TITAN_SYNC_MODE=secondary TITAN_HUB_URL="ws://192.168.1.100:8765/ws" pnpm tauri dev
//! ```
//!
//! ## Architecture
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                    Standalone Hub Server Mode                           │
//! │                                                                         │
//! │                    ┌──────────────────────┐                             │
//! │                    │   hub-server         │                             │
//! │                    │   (this binary)      │                             │
//! │                    │   Port 8765          │                             │
//! │                    └──────────┬───────────┘                             │
//! │                               │                                         │
//! │         ┌─────────────────────┼─────────────────────┐                   │
//! │         ▼                     ▼                     ▼                   │
//! │  ┌──────────────┐     ┌──────────────┐      ┌──────────────┐           │
//! │  │    POS #1    │     │    POS #2    │      │    POS #3    │           │
//! │  │ (secondary)  │     │ (secondary)  │      │ (secondary)  │           │
//! │  └──────────────┘     └──────────────┘      └──────────────┘           │
//! └─────────────────────────────────────────────────────────────────────────┘
//!
//! With Cloud Sync:
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │  LOCAL                                    CLOUD                         │
//! │  ┌──────────────────────┐                ┌─────────────────────────┐   │
//! │  │   hub-server         │───── gRPC ────▶│   Cloud API (50051)     │   │
//! │  │   + CloudUplink      │                │   PostgreSQL backend    │   │
//! │  └──────────┬───────────┘                └─────────────────────────┘   │
//! │             │                                                           │
//! │   POS terminals connect here                                            │
//! └─────────────────────────────────────────────────────────────────────────┘
//! ```

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
use tokio::time::Duration;
use tracing::{error, info, warn, Level};
use tracing_subscriber::FmtSubscriber;

// Import from titan_sync library
use titan_sync::{
    HubCore, CloudUplinkCallback,
    CloudUplink, CloudUplinkConfig, CloudUplinkAdapter,
    protocol::{SyncMessage, HelloPayload, WelcomePayload},
};

// =============================================================================
// Server Configuration
// =============================================================================

/// Hub server configuration parsed from command line
struct ServerConfig {
    port: u16,
    store_id: String,
    bind_addr: String,
    cloud_url: Option<String>,
    tenant_id: Option<String>,
    api_key: Option<String>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            port: 8765,
            store_id: "demo-store".to_string(),
            bind_addr: "0.0.0.0".to_string(),
            cloud_url: None,
            tenant_id: None,
            api_key: None,
        }
    }
}

// =============================================================================
// Application State
// =============================================================================

/// Shared application state wrapping HubCore
struct AppState {
    hub: HubCore,
}

// =============================================================================
// WebSocket Handlers
// =============================================================================

async fn health_handler() -> impl IntoResponse {
    "OK"
}

async fn stats_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let stats = state.hub.stats().await;
    serde_json::to_string(&stats).unwrap_or_else(|_| r#"{"error":"serialization failed"}"#.to_string())
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<SocketAddr>,
) -> impl IntoResponse {
    info!(addr = %addr, "New WebSocket connection");
    ws.on_upgrade(move |socket| handle_socket(socket, state, addr))
}

async fn handle_socket(socket: WebSocket, state: Arc<AppState>, addr: SocketAddr) {
    let (mut sender, mut receiver) = socket.split();

    // Wait for Hello message
    let hello = match wait_for_hello(&mut receiver).await {
        Ok(h) => h,
        Err(e) => {
            warn!(addr = %addr, error = %e, "Failed to receive Hello");
            return;
        }
    };

    let device_id = hello.device_id.clone();
    let device_name = hello.device_name.clone();
    let client_store_id = hello.store_id.clone();

    // Register client with HubCore (validates store_id internally)
    let (_client_tx, mut client_rx, mut broadcast_rx) = match state.hub.register_client(
        &device_id,
        &device_name,
        &client_store_id,
        addr,
    ).await {
        Ok(channels) => channels,
        Err(e) => {
            warn!(
                device_id = %device_id,
                error = ?e,
                "Failed to register client"
            );
            // Send error message
            let error = SyncMessage::Error {
                code: "REGISTRATION_FAILED".to_string(),
                message: e.to_string(),
            };
            if let Ok(json) = serde_json::to_string(&error) {
                let _ = sender.send(Message::Text(json.into())).await;
            }
            return;
        }
    };

    info!(device_id = %device_id, store_id = %client_store_id, addr = %addr, "Client authenticated");

    // Send Welcome
    let welcome = SyncMessage::Welcome(WelcomePayload {
        hub_device_id: "hub-server".to_string(),
        store_id: state.hub.store_id().to_string(),
        server_time: chrono::Utc::now().to_rfc3339(),
        election_term: 1,
    });
    if let Ok(json) = serde_json::to_string(&welcome) {
        if sender.send(Message::Text(json.into())).await.is_err() {
            warn!(device_id = %device_id, "Failed to send Welcome");
            state.hub.remove_client(&device_id).await;
            return;
        }
    }

    // Message handling loop
    let device_id_clone = device_id.clone();
    let state_clone = state.clone();
    
    loop {
        tokio::select! {
            // Message from client
            msg = receiver.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        if let Err(e) = handle_client_message(&state_clone, &device_id_clone, &text).await {
                            warn!(device_id = %device_id_clone, error = ?e, "Error handling message");
                        }
                    }
                    Some(Ok(Message::Ping(data))) => {
                        if sender.send(Message::Pong(data)).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        info!(device_id = %device_id_clone, "Client disconnected");
                        break;
                    }
                    Some(Err(e)) => {
                        warn!(device_id = %device_id_clone, error = %e, "WebSocket error");
                        break;
                    }
                    _ => {}
                }
            }
            // Broadcast messages from hub
            msg = broadcast_rx.recv() => {
                if let Ok(text) = msg {
                    if sender.send(Message::Text(text.into())).await.is_err() {
                        break;
                    }
                }
            }
            // Direct messages to this client
            msg = client_rx.recv() => {
                if let Some(text) = msg {
                    if sender.send(Message::Text(text.into())).await.is_err() {
                        break;
                    }
                }
            }
        }
    }

    state.hub.remove_client(&device_id).await;
}

async fn wait_for_hello(
    receiver: &mut futures_util::stream::SplitStream<WebSocket>,
) -> Result<HelloPayload, String> {
    let timeout = tokio::time::timeout(Duration::from_secs(10), async {
        while let Some(msg) = receiver.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    if let Ok(SyncMessage::Hello(hello)) = serde_json::from_str(&text) {
                        return Ok(hello);
                    }
                }
                Ok(Message::Ping(_)) => continue,
                _ => {}
            }
        }
        Err("Connection closed before Hello".to_string())
    });

    timeout.await.map_err(|_| "Timeout waiting for Hello".to_string())?
}

async fn handle_client_message(
    state: &Arc<AppState>,
    device_id: &str,
    text: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let msg: SyncMessage = serde_json::from_str(text)?;
    
    // Delegate to HubCore for message handling
    if let Some(response) = state.hub.handle_message(device_id, msg).await? {
        // Send response back to the client
        state.hub.send_to_client(device_id, &response).await?;
    }
    
    Ok(())
}

// =============================================================================
// Cloud Uplink Setup
// =============================================================================

async fn setup_cloud_uplink(config: &ServerConfig) -> Option<Arc<dyn CloudUplinkCallback>> {
    let cloud_url = config.cloud_url.as_ref()?;
    let tenant_id = config.tenant_id.as_ref()?;
    let api_key = config.api_key.as_ref()?;

    info!(
        cloud_url = %cloud_url,
        tenant_id = %tenant_id,
        "Connecting to cloud API..."
    );

    let uplink_config = CloudUplinkConfig {
        cloud_url: cloud_url.clone(),
        store_id: config.store_id.clone(),
        tenant_id: tenant_id.clone(),
        api_key: api_key.clone(),
        device_id: format!("hub-{}", config.store_id),
        device_name: Some(format!("Hub Server ({})", config.store_id)),
        verify_tls: false, // Allow self-signed certs for local dev
        ..Default::default()
    };

    match CloudUplink::new(uplink_config) {
        Ok(mut uplink) => {
            match uplink.connect().await {
                Ok(()) => {
                    info!("✅ Connected to cloud API");
                    let adapter = CloudUplinkAdapter::new(uplink);
                    Some(Arc::new(adapter) as Arc<dyn CloudUplinkCallback>)
                }
                Err(e) => {
                    error!(?e, "❌ Failed to connect to cloud API - running without cloud sync");
                    None
                }
            }
        }
        Err(e) => {
            error!(?e, "❌ Failed to create cloud uplink - running without cloud sync");
            None
        }
    }
}

// =============================================================================
// CLI Parsing
// =============================================================================

fn parse_args() -> Option<ServerConfig> {
    let args: Vec<String> = std::env::args().collect();
    let mut config = ServerConfig::default();

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--port" | "-p" => {
                if i + 1 < args.len() {
                    config.port = args[i + 1].parse().unwrap_or(8765);
                    i += 1;
                }
            }
            "--store-id" | "-s" => {
                if i + 1 < args.len() {
                    config.store_id = args[i + 1].clone();
                    i += 1;
                }
            }
            "--bind" | "-b" => {
                if i + 1 < args.len() {
                    config.bind_addr = args[i + 1].clone();
                    i += 1;
                }
            }
            "--cloud-url" | "-c" => {
                if i + 1 < args.len() {
                    config.cloud_url = Some(args[i + 1].clone());
                    i += 1;
                }
            }
            "--tenant-id" | "-t" => {
                if i + 1 < args.len() {
                    config.tenant_id = Some(args[i + 1].clone());
                    i += 1;
                }
            }
            "--api-key" | "-k" => {
                if i + 1 < args.len() {
                    config.api_key = Some(args[i + 1].clone());
                    i += 1;
                }
            }
            "--help" | "-h" => {
                print_help();
                return None;
            }
            _ => {}
        }
        i += 1;
    }

    Some(config)
}

fn print_help() {
    println!("Titan POS Standalone Hub Server");
    println!();
    println!("USAGE:");
    println!("    hub-server [OPTIONS]");
    println!();
    println!("OPTIONS:");
    println!("    -p, --port <PORT>         Port to listen on [default: 8765]");
    println!("    -s, --store-id <ID>       Store ID to accept [default: demo-store]");
    println!("    -b, --bind <ADDR>         Bind address [default: 0.0.0.0]");
    println!();
    println!("CLOUD SYNC OPTIONS:");
    println!("    -c, --cloud-url <URL>     Cloud API URL (e.g., http://localhost:50051)");
    println!("    -t, --tenant-id <ID>      Tenant ID for cloud auth");
    println!("    -k, --api-key <KEY>       API key for cloud auth");
    println!();
    println!("    -h, --help                Print help");
    println!();
    println!("EXAMPLES:");
    println!("    # Local-only hub (no cloud sync)");
    println!("    hub-server --port 8765 --store-id my-store");
    println!();
    println!("    # Hub with cloud sync");
    println!("    hub-server --port 8765 --store-id my-store \\");
    println!("        --cloud-url http://localhost:50051 \\");
    println!("        --tenant-id demo-tenant \\");
    println!("        --api-key demo-api-key");
    println!();
    println!("Then connect POS terminals:");
    println!("    TITAN_SYNC_MODE=secondary TITAN_HUB_URL=\"ws://192.168.1.100:8765/ws\" pnpm tauri dev");
}

// =============================================================================
// Main
// =============================================================================

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    let _subscriber = FmtSubscriber::builder()
        .with_max_level(Level::DEBUG)
        .with_target(true)
        .pretty()
        .init();

    // Parse command line args
    let config = match parse_args() {
        Some(c) => c,
        None => return Ok(()), // --help was printed
    };

    info!(
        port = config.port,
        store_id = %config.store_id,
        bind_addr = %config.bind_addr,
        cloud_enabled = config.cloud_url.is_some(),
        "Starting Titan Hub Server"
    );

    // Setup cloud uplink if configured
    let cloud_uplink = setup_cloud_uplink(&config).await;
    
    // Create hub core with or without cloud uplink
    let hub = if let Some(uplink) = cloud_uplink {
        info!("🌐 Cloud sync enabled");
        HubCore::with_cloud_uplink(config.store_id.clone(), uplink)
    } else {
        info!("📦 Running in local-only mode (no cloud sync)");
        HubCore::new(config.store_id.clone())
    };

    // Create application state
    let state = Arc::new(AppState { hub });

    // Build router
    let app = Router::new()
        .route("/ws", get(ws_handler))
        .route("/health", get(health_handler))
        .route("/stats", get(stats_handler))
        .with_state(state)
        .into_make_service_with_connect_info::<SocketAddr>();

    // Start server
    let addr = format!("{}:{}", config.bind_addr, config.port);
    let listener = TcpListener::bind(&addr).await?;
    
    info!(addr = %addr, "Hub server listening");
    info!("WebSocket endpoint: ws://{}:{}/ws", config.bind_addr, config.port);
    info!("Health endpoint: http://{}:{}/health", config.bind_addr, config.port);
    info!("Stats endpoint: http://{}:{}/stats", config.bind_addr, config.port);
    info!("");
    info!("Connect POS terminals with:");
    info!("  TITAN_SYNC_MODE=secondary TITAN_HUB_URL=\"ws://{}:{}/ws\" pnpm tauri dev", 
        if config.bind_addr == "0.0.0.0" { "<YOUR_IP>" } else { &config.bind_addr }, 
        config.port
    );

    axum::serve(listener, app).await?;

    Ok(())
}
