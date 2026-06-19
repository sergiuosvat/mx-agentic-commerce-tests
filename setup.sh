#!/bin/bash
set -e
set -o pipefail

# ==============================================================================
# Agentic Commerce Tests — Full Environment Setup
# Clone, build, and configure everything from scratch.
# Assumes: git, node 18+, npm, cargo installed.
# ==============================================================================

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(dirname "$SCRIPT_DIR")"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

ok()   { echo -e "${GREEN}✓${NC} $1"; }
warn() { echo -e "${YELLOW}⚠${NC} $1"; }
fail() { echo -e "${RED}✗${NC} $1"; exit 1; }
info() { echo -e "${CYAN}ℹ${NC} $1"; }

echo "============================================"
echo " Agentic Commerce Tests — Full Setup"
echo "============================================"
echo ""

# ── 1. Prerequisites ──────────────────────────────────────────────────────────

echo "▶ Checking prerequisites..."

command -v git   >/dev/null 2>&1 || fail "git not found"
command -v node  >/dev/null 2>&1 || fail "node not found. Install Node.js v18+"
command -v npm   >/dev/null 2>&1 || fail "npm not found. Install Node.js v18+"
command -v cargo >/dev/null 2>&1 || fail "cargo not found. Install Rust toolchain"

NODE_MAJOR=$(node -v | sed 's/v//' | cut -d. -f1)
[ "$NODE_MAJOR" -ge 18 ] 2>/dev/null || warn "Node.js v18+ recommended (found $(node -v))"

ok "git, node $(node -v), npm $(npm -v), cargo $(cargo --version | cut -d' ' -f2)"

# ── 2. Clone Dependencies (if missing) ───────────────────────────────────────

echo ""
echo "▶ Checking / cloning dependencies..."

# GitHub org for sibling repos
GH_ORG="${GH_ORG:-sasurobert}"
MPP_VERSION="${MPP_VERSION:-0.4.8}"

clone_if_missing() {
    local name="$1"
    local target_dir="$2"
    local repo_url="$3"
    local branch="${4:-master}"

    if [ -d "$target_dir" ]; then
        ok "$name already present"
    else
        info "Cloning $name from $repo_url ..."
        git clone --depth 1 --branch "$branch" "$repo_url" "$target_dir" 2>/dev/null \
            || git clone --depth 1 "$repo_url" "$target_dir"
        ok "$name cloned"
    fi
}

# Smart Contracts
clone_if_missing "mx-8004 (Smart Contracts)" \
    "$ROOT_DIR/mx-8004" \
    "https://github.com/${GH_ORG}/mx-8004.git"

clone_if_missing "mpp-session-mvx (MPP Session)" \
    "$ROOT_DIR/mpp-session-mvx" \
    "https://github.com/${GH_ORG}/mpp-session-mvx.git"

# MCP Server
clone_if_missing "multiversx-mcp-server" \
    "$ROOT_DIR/multiversx-mcp-server" \
    "https://github.com/${GH_ORG}/multiversx-mcp-server.git"

# Moltbot Starter Kit
clone_if_missing "moltbot-starter-kit" \
    "$ROOT_DIR/moltbot-starter-kit" \
    "https://github.com/${GH_ORG}/moltbot-starter-kit.git"

# MPP MultiversX SDK (file:../mppx-multiversx dep in multiversx-mcp-server)
clone_if_missing "mppx-multiversx" \
    "$ROOT_DIR/mppx-multiversx" \
    "https://github.com/${GH_ORG}/mppx-multiversx.git"

# x402 Integration directory
mkdir -p "$ROOT_DIR/x402_integration"

# x402 Facilitator
clone_if_missing "x402-facilitator" \
    "$ROOT_DIR/x402_integration/x402_facilitator" \
    "https://github.com/${GH_ORG}/x402-facilitator.git"

# OpenClaw Relayer
clone_if_missing "multiversx-openclaw-relayer" \
    "$ROOT_DIR/x402_integration/multiversx-openclaw-relayer" \
    "https://github.com/${GH_ORG}/multiversx-openclaw-relayer.git"

# Chain Simulator (from MultiversX org)
clone_if_missing "mx-chain-simulator-go" \
    "$ROOT_DIR/mx-chain-simulator-go" \
    "https://github.com/multiversx/mx-chain-simulator-go.git"

# ── 3. Chain Simulator Binary ────────────────────────────────────────────────

echo ""
echo "▶ Checking Chain Simulator binary..."

SIM_BIN=""
if [ -x "$SCRIPT_DIR/mx-chain-simulator-go" ]; then
    SIM_BIN="$SCRIPT_DIR/mx-chain-simulator-go"
elif command -v mx-chain-simulator-go >/dev/null 2>&1; then
    SIM_BIN="$(command -v mx-chain-simulator-go)"
fi

if [ -n "$SIM_BIN" ]; then
    ok "Chain Simulator found: $SIM_BIN"
else
    if command -v go >/dev/null 2>&1 && [ -d "$ROOT_DIR/mx-chain-simulator-go" ]; then
        info "Building Chain Simulator from source..."
        (cd "$ROOT_DIR/mx-chain-simulator-go/cmd/chainsimulator" && go build -o "$SCRIPT_DIR/mx-chain-simulator-go" .)
        ok "Chain Simulator built: $SCRIPT_DIR/mx-chain-simulator-go"
    else
        warn "Chain Simulator not found. Install Go 1.20+ to build it automatically."
    fi
fi

# ── 4. Smart Contract WASM Artifacts ────────────────────────────────────────

echo ""
echo "▶ Checking WASM artifacts..."

mkdir -p "$SCRIPT_DIR/artifacts"

WASM_OK=true
for contract in identity-registry validation-registry reputation-registry; do
    WASM_FILE="$SCRIPT_DIR/artifacts/$contract.wasm"
    if [ -f "$WASM_FILE" ]; then
        ok "$contract.wasm ($(wc -c < "$WASM_FILE" | tr -d ' ') bytes)"
    else
        # Try to copy from mx-8004/output
        SRC="$ROOT_DIR/mx-8004/$contract/output/$contract.wasm"
        if [ -f "$SRC" ]; then
            cp "$SRC" "$WASM_FILE"
            ok "$contract.wasm copied from mx-8004 output"
        else
            WASM_OK=false
            warn "$contract.wasm missing — will try to build"
        fi
    fi
done

if [ "$WASM_OK" = false ]; then
    if command -v sc-meta >/dev/null 2>&1; then
        info "Building contracts with sc-meta..."
        (cd "$ROOT_DIR/mx-8004" && sc-meta all build)
        for contract in identity-registry validation-registry reputation-registry; do
            SRC="$ROOT_DIR/mx-8004/$contract/output/$contract.wasm"
            [ -f "$SRC" ] && cp "$SRC" "$SCRIPT_DIR/artifacts/"
        done
        ok "Contracts built and copied"
    else
        warn "sc-meta not found. Install with: cargo install multiversx-sc-meta"
        warn "Then run: cd $ROOT_DIR/mx-8004 && sc-meta all build"
    fi
fi

# Copy mxsc.json files too
for contract in identity-registry validation-registry reputation-registry; do
    MXSC_SRC="$ROOT_DIR/mx-8004/$contract/output/$contract.mxsc.json"
    MXSC_DST="$SCRIPT_DIR/artifacts/$contract.mxsc.json"
    [ -f "$MXSC_SRC" ] && [ ! -f "$MXSC_DST" ] && cp "$MXSC_SRC" "$MXSC_DST"
done

SESSION_MXSC="$ROOT_DIR/mpp-session-mvx/output/mpp-session-mvx.mxsc.json"
if [ -f "$SESSION_MXSC" ]; then
    ok "mpp-session-mvx.mxsc.json ($(wc -c < "$SESSION_MXSC" | tr -d ' ') bytes)"
elif command -v sc-meta >/dev/null 2>&1 && [ -d "$ROOT_DIR/mpp-session-mvx" ]; then
    info "Building mpp-session-mvx contract..."
    (cd "$ROOT_DIR/mpp-session-mvx" && sc-meta all build)
    [ -f "$SESSION_MXSC" ] || fail "mpp-session-mvx build did not produce $SESSION_MXSC"
    ok "mpp-session-mvx built"
else
    fail "mpp-session-mvx artifact missing at $SESSION_MXSC (clone repo or install sc-meta)"
fi

MPP_PROXY_SRC="$ROOT_DIR/mpp-session-mvx/interactor/src/mpp_session_mvx_proxy.rs"
MPP_PROXY_DST="$SCRIPT_DIR/tests/common/mpp_session_mvx_proxy.rs"
if [ -d "$ROOT_DIR/mpp-session-mvx/meta" ]; then
    info "Generating mpp-session-mvx proxy for tests..."
    (cd "$ROOT_DIR/mpp-session-mvx/meta" && cargo run --quiet -- proxy)
    [ -f "$MPP_PROXY_SRC" ] || fail "Proxy generation did not produce $MPP_PROXY_SRC"
    cp "$MPP_PROXY_SRC" "$MPP_PROXY_DST"
    ok "mpp_session_mvx_proxy.rs -> tests/common/"
else
    fail "mpp-session-mvx/meta not found — cannot generate test proxy"
fi

# ── 5. Node.js Services ─────────────────────────────────────────────────────

echo ""
echo "▶ Building MPP packages..."

ensure_mppx_ready() {
    local mppx_dir="$ROOT_DIR/mppx"

    if [ -f "$mppx_dir/dist/server/index.js" ] \
        && grep -q "\"version\": \"${MPP_VERSION}\"" "$mppx_dir/package.json" 2>/dev/null; then
        ok "mppx@${MPP_VERSION} ready"
        return
    fi

    info "Installing mppx@${MPP_VERSION} from npm..."
    local staging
    staging=$(mktemp -d)
    (cd "$staging" && npm pack "mppx@${MPP_VERSION}" --silent)
    rm -rf "$mppx_dir"
    mkdir -p "$mppx_dir"
    tar -xzf "$staging"/mppx-*.tgz -C "$staging"
    cp -a "$staging/package/." "$mppx_dir/"
    rm -rf "$staging"
    ok "mppx@${MPP_VERSION} installed"
}

ensure_mppx_ready

if [ -d "$ROOT_DIR/mppx-multiversx" ]; then
    info "Building mppx-multiversx..."
    (cd "$ROOT_DIR/mppx-multiversx" && npm install --silent && npm run build --silent)
    ok "mppx-multiversx built"
else
    warn "mppx-multiversx not found — multiversx-mcp-server build may fail"
fi

echo ""
echo "▶ Building Node.js services..."

build_node_service() {
    local name="$1"
    local dir="$2"

    if [ ! -d "$dir" ]; then
        warn "$name: directory not found at $dir — skipping"
        return
    fi

    info "Building $name..."
    (cd "$dir" && npm install --silent 2>/dev/null && npm run build --silent 2>/dev/null)
    ok "$name built"
}

build_node_service "moltbot-starter-kit"    "$ROOT_DIR/moltbot-starter-kit"
build_node_service "multiversx-mcp-server"  "$ROOT_DIR/multiversx-mcp-server"
build_node_service "openclaw-relayer"       "$ROOT_DIR/x402_integration/multiversx-openclaw-relayer"
build_node_service "x402-facilitator"       "$ROOT_DIR/x402_integration/x402_facilitator"

# ── 6. Rust Test Build ──────────────────────────────────────────────────────

echo ""
echo "▶ Building Rust test suite..."
(cd "$SCRIPT_DIR" && cargo build --tests)
ok "Rust tests compiled"

# ── Summary ──────────────────────────────────────────────────────────────────

echo ""
echo "============================================"
echo "  Setup Complete!"
echo "============================================"
echo ""
echo "Run all tests:"
echo "  cd $SCRIPT_DIR && cargo test -- --nocapture"
echo ""
echo "Run a specific suite:"
echo "  cargo test --test suite_a_identity -- --nocapture"
echo ""
echo "Override GitHub org (for forks):"
echo "  GH_ORG=youruser ./setup.sh"
echo ""
echo "Override MPP core version (default 0.4.8):"
echo "  MPP_VERSION=0.4.12 ./setup.sh"
echo ""
