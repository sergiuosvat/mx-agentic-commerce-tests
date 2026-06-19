#!/usr/bin/env bash
#
# run_all_tests.sh — Full Ecosystem Test Runner
#
# Runs integration tests sequentially with port cleanup, retry logic, and timing.
#
# Usage:
#   ./run_all_tests.sh              # TS unit + chain-sim suites + off-chain suites
#   ./run_all_tests.sh --suites     # Chain-sim suites only (A–T, Z, session)
#   ./run_all_tests.sh --offchain   # Off-chain suites only (U–Y)
#   ./run_all_tests.sh --ts         # TypeScript tests (unit + optional chain)
#   ./run_all_tests.sh --rust       # mx-8004 smart contract tests
#   ./run_all_tests.sh --pkg        # Package-level tests (pkg_1–9, chain-only)
#   ./run_all_tests.sh --all        # Everything including pkg + sibling TS services
#
# Environment:
#   RUN_CHAIN_TESTS=1               Enable on-chain MPP payment test (needs funded keys)
#   FAIL_ON_RETRY=1                 Exit non-zero if any suite passed only after retry

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
SIM_PORT=8085
MAX_PORT_WAIT=30
RETRY_DELAY=5
COOLDOWN=3
LOG_DIR="$SCRIPT_DIR/target/test-logs"
FAIL_ON_RETRY="${FAIL_ON_RETRY:-0}"
CARGO_TEST_FLAGS="-- --test-threads=1 --nocapture"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
BOLD='\033[1m'
NC='\033[0m'

declare -a RESULTS=()
PASS_COUNT=0
FAIL_COUNT=0
RETRY_COUNT=0
RETRY_PASS_COUNT=0

mkdir -p "$LOG_DIR"

log_header() {
    echo ""
    echo -e "${BLUE}═══════════════════════════════════════════════════════════${NC}"
    echo -e "${BOLD}  $1${NC}"
    echo -e "${BLUE}═══════════════════════════════════════════════════════════${NC}"
}

log_suite() {
    echo -e "\n${BOLD}▸ $1${NC}"
}

log_pass() {
    echo -e "  ${GREEN}✅ PASS${NC} ($1)"
}

log_fail() {
    echo -e "  ${RED}❌ FAIL${NC} ($1)"
}

log_retry() {
    echo -e "  ${YELLOW}🔄 RETRY${NC} ($1)"
}

kill_test_processes() {
    pkill -f "mx-chain-simulator-go" 2>/dev/null || true
    for port in $SIM_PORT 3000 3001 3002 3004 3006 4000; do
        lsof -ti:"$port" 2>/dev/null | xargs kill -9 2>/dev/null || true
    done
}

wait_for_port_free() {
    local port=$1
    local max_wait=${2:-$MAX_PORT_WAIT}
    local waited=0

    while [ "$waited" -lt "$max_wait" ]; do
        if ! nc -z 127.0.0.1 "$port" 2>/dev/null; then
            return 0
        fi
        sleep 1
        waited=$((waited + 1))
    done

    echo -e "  ${YELLOW}⚠ Port $port still occupied after ${max_wait}s${NC}"
    lsof -ti:"$port" 2>/dev/null | xargs kill -9 2>/dev/null || true
    sleep 2
}

# $1=name $2=command $3=cwd
run_test() {
    local name="$1"
    local cmd="$2"
    local cwd="${3:-$SCRIPT_DIR}"
    local slug
    slug="$(echo "$name" | tr ' /—' '___' | tr -cd '[:alnum:]_-')"
    local log_file="$LOG_DIR/${slug}.log"
    local start_time end_time duration exit_code

    log_suite "$name"

    start_time=$(date +%s)
    set +e
    (cd "$cwd" && eval "$cmd") >"$log_file" 2>&1
    exit_code=$?
    set -e
    end_time=$(date +%s)
    duration="$((end_time - start_time))s"

    if [ "$exit_code" -eq 0 ]; then
        log_pass "$duration"
        RESULTS+=("${GREEN}✅${NC} | $name | $duration")
        PASS_COUNT=$((PASS_COUNT + 1))
        return 0
    fi

    log_retry "failed, retrying after cleanup (see $log_file)..."
    RETRY_COUNT=$((RETRY_COUNT + 1))

    kill_test_processes
    wait_for_port_free "$SIM_PORT"
    sleep "$RETRY_DELAY"

    start_time=$(date +%s)
    set +e
    (cd "$cwd" && eval "$cmd") >"${log_file%.log}.retry.log" 2>&1
    exit_code=$?
    set -e
    end_time=$(date +%s)
    duration="$((end_time - start_time))s"

    if [ "$exit_code" -eq 0 ]; then
        log_pass "$duration (retry)"
        RESULTS+=("${YELLOW}🔄${NC} | $name | $duration (retry)")
        PASS_COUNT=$((PASS_COUNT + 1))
        RETRY_PASS_COUNT=$((RETRY_PASS_COUNT + 1))
        return 0
    fi

    log_fail "$duration"
    echo "  Last 50 lines of output:"
    tail -50 "${log_file%.log}.retry.log" 2>/dev/null | sed 's/^/    /'
    echo "  Deployed / health hints from log:"
    grep -E "Deployed|started on port|health|localhost:[0-9]+" "${log_file%.log}.retry.log" 2>/dev/null | tail -15 | sed 's/^/    /' || true
    RESULTS+=("${RED}❌${NC} | $name | $duration")
    FAIL_COUNT=$((FAIL_COUNT + 1))
    return 1
}

between_suites() {
    wait_for_port_free "$SIM_PORT" 10
    sleep "$COOLDOWN"
}

run_mpp_typescript_tests() {
    log_header "MPP TypeScript Tests (this repo)"

    if [ ! -d "$SCRIPT_DIR/node_modules" ]; then
        info="Installing npm dependencies..."
        echo -e "  ${BLUE}ℹ${NC} $info"
        (cd "$SCRIPT_DIR" && npm install --silent)
    fi

    run_test "TS: MPP unit (402 challenge)" "npm run test:unit" "$SCRIPT_DIR" || true

    if [ "${RUN_CHAIN_TESTS:-0}" = "1" ]; then
        run_test "TS: MPP chain payment" "npm run test:chain" "$SCRIPT_DIR" || true
    else
        echo -e "  ${YELLOW}⏭ Skipped MPP chain test (set RUN_CHAIN_TESTS=1 to enable)${NC}"
    fi
}

run_typescript_service_tests() {
    log_header "TypeScript Service Unit Tests (sibling repos)"

    local services=(
        "moltbot-starter-kit|npm test|$ROOT_DIR/moltbot-starter-kit"
        "x402_facilitator|npm test|$ROOT_DIR/x402_integration/x402_facilitator"
        "multiversx-openclaw-relayer|npm test|$ROOT_DIR/x402_integration/multiversx-openclaw-relayer"
        "multiversx-acp-adapter|npm test|$ROOT_DIR/multiversx-acp-adapter"
        "multiversx-mcp-server|npm test|$ROOT_DIR/multiversx-mcp-server"
        "multiversx-openclaw-skills|npm test|$ROOT_DIR/multiversx-openclaw-skills"
    )

    for svc in "${services[@]}"; do
        IFS='|' read -r name cmd cwd <<< "$svc"
        if [ -d "$cwd" ]; then
            run_test "TS: $name" "$cmd" "$cwd" || true
        else
            echo -e "  ${YELLOW}⏭ Skipped $name (dir not found)${NC}"
        fi
    done
}

run_rust_sc_tests() {
    log_header "Rust Smart Contract Tests (mx-8004)"

    local mx8004_dir="$ROOT_DIR/mx-8004"
    if [ -d "$mx8004_dir" ]; then
        run_test "Rust SC: mx-8004" "cargo test $CARGO_TEST_FLAGS" "$mx8004_dir" || true
    fi
}

run_chain_sim_suites() {
    log_header "Chain-Sim Integration Suites (A–T, Z, session)"

    kill_test_processes
    wait_for_port_free "$SIM_PORT"
    sleep 2

    local suites=(
        "Suite A — Identity Registry|suite_a_identity"
        "Suite D — Facilitator|suite_d_facilitator"
        "Suite E — Moltbot Lifecycle|suite_e_moltbot_lifecycle"
        "Suite E2 — Moltbot Update|suite_e2_moltbot_update"
        "Suite F — Multi Agent|suite_f_multi_agent"
        "Suite G — MCP Features|suite_g_mcp_features"
        "Suite H — Relayed Registration|suite_h_relayed_registration"
        "Suite I — Relayed Agent Ops|suite_i_relayed_agent_ops"
        "Suite J — Relayed Facilitator Settle|suite_j_relayed_facilitator_settle"
        "Suite K — Relayed Moltbot Lifecycle|suite_k_relayed_moltbot_lifecycle"
        "Suite L — MCP Agent Discovery|suite_l_mcp_agent_discovery"
        "Suite M — Agent to Agent Flow|suite_m_agent_to_agent_flow"
        "Suite N — Reputation Validation|suite_n_reputation_validation"
        "Suite O — MCP Tool Coverage|suite_o_mcp_tool_coverage"
        "Suite P — Identity Extended|suite_p_identity_extended"
        "Suite Q — Validation Extended|suite_q_validation_extended"
        "Suite R — Reputation Extended|suite_r_reputation_extended"
        "Suite S — Full Economy Loop|suite_s_full_economy_loop"
        "Suite T — MCP Extended|suite_t_mcp_extended"
        "Suite Z — MPP Facilitator|suite_z_mpp_facilitator"
        "Suite Session — Simulator|suite_session_simulator"
    )

    for suite in "${suites[@]}"; do
        IFS='|' read -r name test_name <<< "$suite"
        run_test "$name" "cargo test --test $test_name $CARGO_TEST_FLAGS" "$SCRIPT_DIR" || true
        between_suites
    done
}

run_offchain_suites() {
    log_header "Off-Chain Integration Suites (U–Y)"

    kill_test_processes
    wait_for_port_free "$SIM_PORT"
    sleep 2

    local suites=(
        "Suite U — Facilitator Extended|suite_u_facilitator_extended"
        "Suite U2 — Facilitator Advanced|suite_u2_facilitator_advanced"
        "Suite V — Relayer Extended|suite_v_relayer_extended"
        "Suite V2 — Relayer Advanced|suite_v2_relayer_advanced"
        "Suite W — Moltbot Extended|suite_w_moltbot_extended"
        "Suite X — E2E Lifecycle|suite_x_e2e_lifecycle"
        "Suite Y — E2E Flows|suite_y_e2e_flows"
    )

    for suite in "${suites[@]}"; do
        IFS='|' read -r name test_name <<< "$suite"
        run_test "$name" "cargo test --test $test_name $CARGO_TEST_FLAGS" "$SCRIPT_DIR" || true
        between_suites
    done
}

run_package_tests() {
    log_header "Package-Level Tests (pkg_1 – pkg_9, chain-only)"

    kill_test_processes
    wait_for_port_free "$SIM_PORT"
    sleep 2

    local pkgs=(
        "pkg_1 — Identity|pkg_1_identity"
        "pkg_2 — Validation|pkg_2_validation"
        "pkg_3 — Reputation|pkg_3_reputation"
        "pkg_4 — Facilitator|pkg_4_facilitator"
        "pkg_5 — MCP|pkg_5_mcp"
        "pkg_6 — Moltbot|pkg_6_moltbot"
        "pkg_7 — E2E|pkg_7_e2e"
        "pkg_8 — Escrow|pkg_8_escrow"
        "pkg_9 — Escrow Lifecycle|pkg_9_escrow_lifecycle"
    )

    for pkg in "${pkgs[@]}"; do
        IFS='|' read -r name test_name <<< "$pkg"
        run_test "$name" "cargo test --test $test_name $CARGO_TEST_FLAGS" "$SCRIPT_DIR" || true
        between_suites
    done
}

print_summary() {
    log_header "Test Results Summary"

    echo ""
    printf "%-4s | %-40s | %s\n" "   " "Test" "Duration"
    echo "-----+------------------------------------------+-----------"
    for result in "${RESULTS[@]}"; do
        echo -e "$result"
    done
    echo ""
    echo -e "${BOLD}Total:${NC} $((PASS_COUNT + FAIL_COUNT)) tests"
    echo -e "${GREEN}Passed:${NC} $PASS_COUNT"
    if [ "$FAIL_COUNT" -gt 0 ]; then
        echo -e "${RED}Failed:${NC} $FAIL_COUNT"
    fi
    if [ "$RETRY_COUNT" -gt 0 ]; then
        echo -e "${YELLOW}Retried:${NC} $RETRY_COUNT (${RETRY_PASS_COUNT} recovered)"
    fi
    echo -e "${BOLD}Logs:${NC} $LOG_DIR"
    echo ""

    local exit_code=0
    if [ "$FAIL_COUNT" -gt 0 ]; then
        echo -e "${RED}${BOLD}⚠ SOME TESTS FAILED — see logs in $LOG_DIR${NC}"
        exit_code=1
    elif [ "$FAIL_ON_RETRY" = "1" ] && [ "$RETRY_PASS_COUNT" -gt 0 ]; then
        echo -e "${YELLOW}${BOLD}⚠ TESTS PASSED AFTER RETRY — investigate flakiness${NC}"
        exit_code=1
    else
        echo -e "${GREEN}${BOLD}🎉 ALL TESTS PASSED!${NC}"
    fi

    return "$exit_code"
}

TOTAL_START=$(date +%s)

case "${1:-default}" in
    --suites)
        run_chain_sim_suites
        ;;
    --offchain)
        run_offchain_suites
        ;;
    --ts)
        run_mpp_typescript_tests
        ;;
    --rust)
        run_rust_sc_tests
        ;;
    --pkg)
        run_package_tests
        ;;
    --all)
        run_mpp_typescript_tests
        run_typescript_service_tests
        run_rust_sc_tests
        run_chain_sim_suites
        run_offchain_suites
        run_package_tests
        ;;
    default|*)
        # Default: cross-service suites without duplicating pkg_* chain-only tests
        run_mpp_typescript_tests
        run_rust_sc_tests
        run_chain_sim_suites
        run_offchain_suites
        ;;
esac

TOTAL_END=$(date +%s)
TOTAL_DURATION=$((TOTAL_END - TOTAL_START))

print_summary
SUMMARY_EXIT=$?

echo -e "\nTotal wall time: ${BOLD}${TOTAL_DURATION}s${NC} ($((TOTAL_DURATION / 60))m $((TOTAL_DURATION % 60))s)"

kill_test_processes 2>/dev/null || true
exit "$SUMMARY_EXIT"
