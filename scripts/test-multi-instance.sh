#!/bin/bash
# =============================================================================
# test-multi-instance.sh - Run multiple Titan POS instances on a single machine
# =============================================================================
#
# This script allows testing multi-device sync by running two POS instances
# with different configurations (data directories, ports).
#
# Usage:
#   ./scripts/test-multi-instance.sh
#
# What it does:
#   1. Creates two data directories (data/pos1/ and data/pos2/)
#   2. Seeds both with initial data using the Rust seed binary
#   3. Provides instructions for starting both instances
#
# Requirements:
#   - Rust toolchain (for building the seed binary)
#   - Node.js / pnpm
#

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo -e "${BLUE}╔══════════════════════════════════════════════════════════════════╗${NC}"
echo -e "${BLUE}║     Titan POS - Multi-Instance Test Setup                        ║${NC}"
echo -e "${BLUE}╚══════════════════════════════════════════════════════════════════╝${NC}"
echo ""

# Configuration
POS1_DATA_DIR="$PROJECT_ROOT/data/pos1"
POS2_DATA_DIR="$PROJECT_ROOT/data/pos2"
POS1_PORT=8765
POS2_PORT=8766
POS1_VITE_PORT=5173
POS2_VITE_PORT=5174

# Cleanup function
cleanup() {
    if [ $? -ne 0 ]; then
        echo -e "\n${RED}Setup failed. Check the error messages above.${NC}"
    fi
}

# Set trap to cleanup on exit
trap cleanup EXIT

# Step 1: Build the seed binary
echo -e "${GREEN}[1/5] Building seed binary...${NC}"
cd "$PROJECT_ROOT"
DATABASE_URL="sqlite:data/titan.db" cargo build -p titan-db --bin seed --quiet
echo "  ✓ Seed binary built"

# Step 2: Create data directories
echo -e "${GREEN}[2/5] Creating data directories...${NC}"
rm -rf "$POS1_DATA_DIR" "$POS2_DATA_DIR" 2>/dev/null || true
mkdir -p "$POS1_DATA_DIR"
mkdir -p "$POS2_DATA_DIR"
mkdir -p "$POS1_DATA_DIR/logs"
mkdir -p "$POS2_DATA_DIR/logs"
echo "  ✓ Created $POS1_DATA_DIR"
echo "  ✓ Created $POS2_DATA_DIR"

# Step 3: Initialize and seed databases using the Rust seed binary
echo -e "${GREEN}[3/5] Initializing databases with seed data...${NC}"

seed_database() {
    local data_dir=$1
    local db_path="$data_dir/titan.db"
    local instance_name=$2
    
    echo "  Seeding database for $instance_name..."
    
    # Run the seed binary which:
    # 1. Creates the database file if it doesn't exist
    # 2. Runs SQLx migrations (creating _sqlx_migrations table)
    # 3. Inserts ~5000 test products
    DATABASE_URL="sqlite:$db_path" "$PROJECT_ROOT/target/debug/seed" --db "$db_path" --count 500 2>&1 | sed 's/^/    /'
    
    echo "  ✓ $instance_name database ready"
}

seed_database "$POS1_DATA_DIR" "POS 1 (PRIMARY)"
seed_database "$POS2_DATA_DIR" "POS 2 (SECONDARY)"

# Step 4: Create sync config files
echo -e "${GREEN}[4/5] Creating sync configuration files...${NC}"

# POS 1 config (PRIMARY)
cat > "$POS1_DATA_DIR/sync.toml" << EOF
# Titan POS Sync Configuration - Instance 1 (PRIMARY)
# This instance will act as the Store Hub

[device]
id = "device-pos-1"
name = "Register 1 (PRIMARY)"
priority = 100  # Higher priority = more likely to be PRIMARY

[store]
id = "test-store-001"
name = "Test Store"

[sync]
mode = "primary"  # Force PRIMARY mode for testing
batch_size = 100
poll_interval_secs = 5

[hub]
port = $POS1_PORT
broadcast_mode = "coalesced"
coalesce_window_ms = 50

[aggregation]
enabled = true
sales_batch_interval_secs = 60

[failover]
grace_period_secs = 5
EOF

# POS 2 config (SECONDARY)
cat > "$POS2_DATA_DIR/sync.toml" << EOF
# Titan POS Sync Configuration - Instance 2 (SECONDARY)
# This instance will connect to Instance 1

[device]
id = "device-pos-2"
name = "Register 2 (SECONDARY)"
priority = 50  # Lower priority

[store]
id = "test-store-001"
name = "Test Store"

[sync]
mode = "secondary"  # Force SECONDARY mode for testing
hub_url = "ws://127.0.0.1:$POS1_PORT/ws"
batch_size = 100
poll_interval_secs = 5

[hub]
port = $POS2_PORT
broadcast_mode = "coalesced"

[aggregation]
enabled = false  # Only PRIMARY aggregates
EOF

echo "  ✓ Created sync.toml for POS 1 (PRIMARY)"
echo "  ✓ Created sync.toml for POS 2 (SECONDARY)"

# Step 5: Display instructions
echo ""
echo -e "${GREEN}[5/5] Setup complete!${NC}"
echo ""
echo -e "${BLUE}╔══════════════════════════════════════════════════════════════════╗${NC}"
echo -e "${BLUE}║                    TEST INSTRUCTIONS                             ║${NC}"
echo -e "${BLUE}╚══════════════════════════════════════════════════════════════════╝${NC}"
echo ""
echo -e "To run the multi-instance test, open TWO separate terminals:"
echo ""
echo -e "${YELLOW}═══════════════════════════════════════════════════════════════════${NC}"
echo -e "${YELLOW}Terminal 1 - Start PRIMARY (Register 1):${NC}"
echo -e "${YELLOW}═══════════════════════════════════════════════════════════════════${NC}"
echo ""
echo -e "  cd $PROJECT_ROOT/apps/desktop"
echo ""
echo -e "  TITAN_DATA_DIR=\"$POS1_DATA_DIR\" \\"
echo -e "  TITAN_DEVICE_ID=\"device-pos-1\" \\"
echo -e "  TITAN_SYNC_MODE=\"primary\" \\"
echo -e "  TITAN_HUB_PORT=$POS1_PORT \\"
echo -e "  pnpm tauri dev"
echo ""
echo -e "${YELLOW}═══════════════════════════════════════════════════════════════════${NC}"
echo -e "${YELLOW}Terminal 2 - Start SECONDARY (Register 2):${NC}"
echo -e "${YELLOW}═══════════════════════════════════════════════════════════════════${NC}"
echo ""
echo -e "  cd $PROJECT_ROOT/apps/desktop"
echo ""
echo -e "  TITAN_DATA_DIR=\"$POS2_DATA_DIR\" \\"
echo -e "  TITAN_DEVICE_ID=\"device-pos-2\" \\"
echo -e "  TITAN_SYNC_MODE=\"secondary\" \\"
echo -e "  TITAN_HUB_URL=\"ws://127.0.0.1:$POS1_PORT/ws\" \\"
echo -e "  VITE_PORT=$POS2_VITE_PORT \\"
echo -e "  $PROJECT_ROOT/scripts/tauri-dev-multi-instance.sh $POS2_VITE_PORT"
echo ""
echo -e "  OR manually run:"
echo ""
echo -e "  TITAN_DATA_DIR=\"$POS2_DATA_DIR\" \\"
echo -e "  TITAN_DEVICE_ID=\"device-pos-2\" \\"
echo -e "  TITAN_SYNC_MODE=\"secondary\" \\"
echo -e "  TITAN_HUB_URL=\"ws://127.0.0.1:$POS1_PORT/ws\" \\"
echo -e "  VITE_PORT=$POS2_VITE_PORT \\"
echo -e "  pnpm tauri dev"
echo ""
echo -e "${BLUE}═══════════════════════════════════════════════════════════════════${NC}"
echo -e "${BLUE}Test Scenarios:${NC}"
echo -e "${BLUE}═══════════════════════════════════════════════════════════════════${NC}"
echo ""
echo "  1. Make a sale on POS 1, check inventory updates on POS 2"
echo "  2. Make a sale on POS 2, check inventory syncs to POS 1"  
echo "  3. Stop POS 1, verify POS 2 detects disconnect"
echo "  4. Restart POS 1, verify POS 2 reconnects"
echo ""
echo -e "${GREEN}Data Directories:${NC}"
echo "  POS 1: $POS1_DATA_DIR"
echo "  POS 2: $POS2_DATA_DIR"
echo ""
echo -e "${GREEN}Database Files:${NC}"
echo "  POS 1 DB: $POS1_DATA_DIR/titan.db"
echo "  POS 2 DB: $POS2_DATA_DIR/titan.db"
echo ""
echo -e "${GREEN}Quick Verification:${NC}"
echo "  sqlite3 $POS1_DATA_DIR/titan.db \"SELECT COUNT(*) FROM products;\""
echo "  sqlite3 $POS2_DATA_DIR/titan.db \"SELECT COUNT(*) FROM products;\""
echo ""
