#!/bin/bash
# =============================================================================
# run-demo.sh - Simple multi-instance demo for Titan POS
# =============================================================================
#
# This script provides a RELIABLE way to test multi-POS sync on a single machine.
#
# It supports three demo modes:
#   1. PRIMARY only  - Single POS with sync hub enabled
#   2. SECONDARY     - Connect to an existing PRIMARY
#   3. setup         - Initialize databases for multi-instance testing
#
# Usage:
#   ./scripts/run-demo.sh setup                    # Initialize test databases
#   ./scripts/run-demo.sh primary                  # Start as PRIMARY (Terminal 1)  
#   ./scripts/run-demo.sh secondary                # Start as SECONDARY (Terminal 2)
#   ./scripts/run-demo.sh primary --port 8080      # Custom hub port
#   ./scripts/run-demo.sh secondary --hub ws://localhost:8080/sync
#
# =============================================================================

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m'

# Default configuration
DEFAULT_HUB_PORT=8765
DEFAULT_PRIMARY_VITE_PORT=5173
DEFAULT_SECONDARY_VITE_PORT=5174

print_header() {
    echo ""
    echo -e "${BLUE}╔══════════════════════════════════════════════════════════════════╗${NC}"
    echo -e "${BLUE}║              Titan POS - Multi-Instance Demo                     ║${NC}"
    echo -e "${BLUE}╚══════════════════════════════════════════════════════════════════╝${NC}"
    echo ""
}

print_usage() {
    echo -e "${CYAN}Usage:${NC}"
    echo "  $0 setup                       Initialize test databases"
    echo "  $0 primary [options]           Start as PRIMARY hub"
    echo "  $0 secondary [options]         Start as SECONDARY client"
    echo ""
    echo -e "${CYAN}Options:${NC}"
    echo "  --port PORT        Hub port (default: $DEFAULT_HUB_PORT)"
    echo "  --vite-port PORT   Vite dev server port"
    echo "  --hub URL          Hub URL for secondary (default: ws://localhost:$DEFAULT_HUB_PORT/sync)"
    echo "  --device-id ID     Custom device ID"
    echo ""
    echo -e "${CYAN}Quick Start:${NC}"
    echo "  Terminal 1: $0 setup && $0 primary"
    echo "  Terminal 2: $0 secondary"
    echo ""
}

setup_databases() {
    echo -e "${GREEN}Setting up demo databases...${NC}"
    
    cd "$PROJECT_ROOT"
    
    # Clean up old test databases
    rm -f data/titan-primary.db data/titan-primary.db-shm data/titan-primary.db-wal
    rm -f data/titan-secondary.db data/titan-secondary.db-shm data/titan-secondary.db-wal
    rm -f data/store_aggregates.db data/store_aggregates.db-shm data/store_aggregates.db-wal
    
    # Ensure base database exists
    if [ ! -f "data/titan.db" ]; then
        echo -e "${YELLOW}Base database not found. Running seed...${NC}"
        cargo run -p titan-db --bin seed -- --db ./data/titan.db --count 1000
    fi
    
    # Copy base database for both instances
    cp data/titan.db data/titan-primary.db
    cp data/titan.db data/titan-secondary.db
    
    echo -e "${GREEN}✓ Created data/titan-primary.db${NC}"
    echo -e "${GREEN}✓ Created data/titan-secondary.db${NC}"
    echo ""
    echo -e "${CYAN}Next steps:${NC}"
    echo "  Terminal 1: $0 primary"
    echo "  Terminal 2: $0 secondary"
    echo ""
}

run_primary() {
    local hub_port=$DEFAULT_HUB_PORT
    local vite_port=$DEFAULT_PRIMARY_VITE_PORT
    local device_id="primary"
    
    # Parse arguments
    while [[ $# -gt 0 ]]; do
        case $1 in
            --port) hub_port="$2"; shift 2 ;;
            --vite-port) vite_port="$2"; shift 2 ;;
            --device-id) device_id="$2"; shift 2 ;;
            *) shift ;;
        esac
    done
    
    echo -e "${GREEN}Starting PRIMARY instance...${NC}"
    echo -e "  Device ID:  ${CYAN}$device_id${NC}"
    echo -e "  Hub Port:   ${CYAN}$hub_port${NC}"
    echo -e "  Vite Port:  ${CYAN}$vite_port${NC}"
    echo -e "  Database:   ${CYAN}data/titan-$device_id.db${NC}"
    echo ""
    
    # Check if database exists
    if [ ! -f "$PROJECT_ROOT/data/titan-$device_id.db" ]; then
        echo -e "${YELLOW}Database not found. Running setup first...${NC}"
        setup_databases
    fi
    
    cd "$PROJECT_ROOT/apps/desktop"
    
    # Set environment and run
    export TITAN_DEVICE_ID="$device_id"
    export TITAN_SYNC_MODE="primary"
    export TITAN_HUB_PORT="$hub_port"
    export VITE_PORT="$vite_port"
    
    echo -e "${GREEN}Launching Tauri...${NC}"
    echo -e "${YELLOW}Wait for: 'Hub server listening addr=0.0.0.0:$hub_port'${NC}"
    echo ""
    
    pnpm tauri dev
}

run_secondary() {
    local hub_port=$DEFAULT_HUB_PORT
    local vite_port=$DEFAULT_SECONDARY_VITE_PORT
    local device_id="secondary"
    local hub_url=""
    
    # Parse arguments
    while [[ $# -gt 0 ]]; do
        case $1 in
            --port) hub_port="$2"; shift 2 ;;
            --vite-port) vite_port="$2"; shift 2 ;;
            --device-id) device_id="$2"; shift 2 ;;
            --hub) hub_url="$2"; shift 2 ;;
            *) shift ;;
        esac
    done
    
    # Default hub URL if not specified
    if [ -z "$hub_url" ]; then
        hub_url="ws://localhost:$hub_port/sync"
    fi
    
    echo -e "${GREEN}Starting SECONDARY instance...${NC}"
    echo -e "  Device ID:  ${CYAN}$device_id${NC}"
    echo -e "  Hub URL:    ${CYAN}$hub_url${NC}"
    echo -e "  Vite Port:  ${CYAN}$vite_port${NC}"
    echo -e "  Database:   ${CYAN}data/titan-$device_id.db${NC}"
    echo ""
    
    # Check if database exists
    if [ ! -f "$PROJECT_ROOT/data/titan-$device_id.db" ]; then
        echo -e "${YELLOW}Database not found. Running setup first...${NC}"
        setup_databases
    fi
    
    # Check if primary is running
    if ! nc -z localhost $hub_port 2>/dev/null; then
        echo -e "${RED}ERROR: No hub found on port $hub_port${NC}"
        echo -e "${YELLOW}Start the PRIMARY first:${NC}"
        echo "  $0 primary"
        exit 1
    fi
    
    echo -e "${GREEN}✓ Primary hub detected on port $hub_port${NC}"
    
    cd "$PROJECT_ROOT/apps/desktop"
    
    # Update tauri.conf.json for different vite port (temporarily)
    local TAURI_CONFIG="$PROJECT_ROOT/apps/desktop/src-tauri/tauri.conf.json"
    local ORIGINAL_CONFIG=$(cat "$TAURI_CONFIG")
    
    # Restore config on exit
    cleanup() {
        echo "$ORIGINAL_CONFIG" > "$TAURI_CONFIG"
        echo -e "${GREEN}✓ Restored tauri.conf.json${NC}"
    }
    trap cleanup EXIT
    
    # Update devUrl if using non-default port
    if [ "$vite_port" != "$DEFAULT_PRIMARY_VITE_PORT" ]; then
        sed -i '' "s|\"devUrl\": \"http://localhost:[0-9]*\"|\"devUrl\": \"http://localhost:$vite_port\"|g" "$TAURI_CONFIG"
        echo -e "${GREEN}✓ Updated devUrl to port $vite_port${NC}"
    fi
    
    # Set environment and run
    export TITAN_DEVICE_ID="$device_id"
    export TITAN_SYNC_MODE="secondary"
    export TITAN_HUB_URL="$hub_url"
    export VITE_PORT="$vite_port"
    
    echo -e "${GREEN}Launching Tauri...${NC}"
    echo -e "${YELLOW}Wait for: 'Connected to hub' in logs${NC}"
    echo ""
    
    pnpm tauri dev
}

# Main entry point
print_header

case "${1:-}" in
    setup)
        setup_databases
        ;;
    primary)
        shift
        run_primary "$@"
        ;;
    secondary)
        shift
        run_secondary "$@"
        ;;
    -h|--help|"")
        print_usage
        ;;
    *)
        echo -e "${RED}Unknown command: $1${NC}"
        print_usage
        exit 1
        ;;
esac
