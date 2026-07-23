#!/bin/bash
# Sync dev binaries to .dev-binaries/ for Coder workspace mounting
#
# This script:
# 1. Builds the openflows binary (if needed)
# 2. Copies it to .dev-binaries/ for Docker mounting
# 3. Optionally hot-deploys into running Nexus workspace

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
DEV_BINARIES_DIR="${PROJECT_ROOT}/.dev-binaries"
RELEASE_BIN="${PROJECT_ROOT}/target/release/openflows"
HARNESS_BIN="${PROJECT_ROOT}/target/release/openflows-harness"

echo "═══════════════════════════════════════"
echo "  OpenFlows Dev Binary Sync"
echo "═══════════════════════════════════════"
echo ""

# Step 1: Build if needed
if [ ! -f "$RELEASE_BIN" ] || [ ! -f "$HARNESS_BIN" ]; then
    echo "Step 1: Building openflows binaries (release mode)..."
    cd "$PROJECT_ROOT"
    cargo build --release -p openflows -p openflows-harness
    echo "✓ Build complete"
    echo ""
else
    echo "Step 1: Release binaries already built"
    echo "  openflows:        $(du -h "$RELEASE_BIN" | cut -f1)"
    echo "  openflows-harness: $(du -h "$HARNESS_BIN" | cut -f1)"
    echo ""
fi

# Step 2: Sync to .dev-binaries
echo "Step 2: Syncing binaries to .dev-binaries/..."
mkdir -p "$DEV_BINARIES_DIR"
cp -v "$RELEASE_BIN" "$DEV_BINARIES_DIR/openflows"
chmod +x "$DEV_BINARIES_DIR/openflows"
cp -v "$HARNESS_BIN" "$DEV_BINARIES_DIR/openflows-harness"
chmod +x "$DEV_BINARIES_DIR/openflows-harness"
echo "✓ Binaries synced"
echo "  openflows:        $DEV_BINARIES_DIR/openflows"
echo "  openflows-harness: $DEV_BINARIES_DIR/openflows-harness"
echo ""

# Step 3: Optional hot-deploy to running workspace
if command -v docker >/dev/null 2>&1; then
    NEXUS_CONTAINER=$(docker ps --filter "name=openflows-nexus" --format "{{.Names}}" 2>/dev/null | head -1 || echo "")
    if [ -n "$NEXUS_CONTAINER" ]; then
        echo "Step 3: Hot-deploying to running workspace ($NEXUS_CONTAINER)..."
        docker cp "$DEV_BINARIES_DIR/openflows" "$NEXUS_CONTAINER:/usr/local/bin/openflows"
        docker exec "$NEXUS_CONTAINER" chmod +x /usr/local/bin/openflows
        echo "✓ Hot-deployed"
        echo ""
        echo "Tip: Restart the controller in the workspace with:"
        echo "  pkill -f 'openflows run' || true"
        echo "  openflows run"
    else
        echo "Step 3: No running Nexus workspace found (will be used on next workspace start)"
        echo ""
    fi
else
    echo "Step 3: Docker not available; binary will be used on next workspace start"
    echo ""
fi

echo "═══════════════════════════════════════"
echo "✓ Dev binary sync complete"
echo "═══════════════════════════════════════"
