#!/bin/bash
# =============================================================================
# tauri-dev-multi-instance.sh - Run Tauri dev with configurable port
# =============================================================================
#
# This script allows running multiple Tauri dev instances on different ports
# by dynamically updating tauri.conf.json with the correct devUrl.
#
# Usage:
#   ./scripts/tauri-dev-multi-instance.sh [port] [additional args...]
#
# Example:
#   ./scripts/tauri-dev-multi-instance.sh 5174
#   ./scripts/tauri-dev-multi-instance.sh 5173
#

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
TAURI_CONFIG="$PROJECT_ROOT/apps/desktop/src-tauri/tauri.conf.json"
DEFAULT_PORT="5173"

# Get port from first argument, default to 5173
VITE_PORT="${1:-5173}"

# Colors for output
BLUE='\033[0;34m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo -e "${BLUE}Configuring Tauri for VITE_PORT=$VITE_PORT${NC}"

# Store original config for restoration
ORIGINAL_CONFIG=$(cat "$TAURI_CONFIG")

# Update devUrl in config
if [[ "$VITE_PORT" != "$DEFAULT_PORT" ]]; then
    sed -i '' "s|\"devUrl\": \"http://localhost:[0-9]*\"|\"devUrl\": \"http://localhost:$VITE_PORT\"|g" "$TAURI_CONFIG"
    echo -e "${GREEN}✓ Updated devUrl to http://localhost:$VITE_PORT${NC}"
fi

# Cleanup function to restore original config
cleanup() {
    if [[ "$VITE_PORT" != "$DEFAULT_PORT" ]]; then
        echo "$ORIGINAL_CONFIG" > "$TAURI_CONFIG"
        echo -e "${YELLOW}✓ Restored tauri.conf.json${NC}"
    fi
}

# Set trap to restore config on exit
trap cleanup EXIT

# Run tauri dev with the VITE_PORT environment variable
echo -e "${BLUE}Starting Tauri dev on port $VITE_PORT${NC}"
cd "$PROJECT_ROOT/apps/desktop"
VITE_PORT=$VITE_PORT pnpm tauri dev
