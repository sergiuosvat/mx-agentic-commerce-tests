# Agentic Commerce End-to-End Tests

Integration test suite for the MultiversX Agentic Commerce ecosystem: smart contracts (mx-8004), chain simulator, facilitator, relayer, MCP server, and Moltbot MPP flows.

## Prerequisites

- Rust toolchain (`cargo`)
- Node.js 18+
- `mx-chain-simulator-go` (installed by `./setup.sh` or on `PATH`)
- Sibling repos cloned by `./setup.sh` (mx-8004, x402 facilitator, moltbot-starter-kit, etc.)

## Quick Start

```bash
./setup.sh
```

## Running Tests

Integration tests bind fixed off-chain ports and start a chain simulator per target. **Always run with a single test thread:**

```bash
cargo test --test pkg_1_identity -- --test-threads=1 --nocapture
# or use the alias:
cargo itest --test pkg_1_identity
```

### Test tiers

| Tier | Command | What it runs |
|------|---------|--------------|
| **Smoke** | `cargo itest --test pkg_1_identity` | Single package, chain-only |
| **Default** | `./run_all_tests.sh` | MPP TS unit + mx-8004 SC + suites A–T/Z/session + off-chain U–Y |
| **Package** | `./run_all_tests.sh --pkg` | Chain-only pkg_1–9 (no sibling services) |
| **Full** | `./run_all_tests.sh --all` | Default + pkg tests + sibling repo npm tests |

### TypeScript (MPP)

```bash
npm install
npm run test:unit          # 402 challenge test (no chain required)
RUN_CHAIN_TESTS=1 npm run test:chain   # on-chain payment (needs funded test keys)
```

### Logs

`run_all_tests.sh` writes per-suite logs to `target/test-logs/`. Set `FAIL_ON_RETRY=1` to fail when a suite only passed after retry.

## Structure

```
tests/
  common/           Shared interactors, simulator helpers, TestEnv harness
  pkg_1–9/          Chain-focused tests by component
  suite_a–z/        Cross-service integration flows
  fixtures/         TypeScript test fixtures (MPP middleware)
  mpp-402.test.ts   MCP 402 challenge (unit, no chain)
  mpp-payment.test.ts  On-chain MPP payment (RUN_CHAIN_TESTS=1)
```

### Package vs suite tests

| Layer | Targets | Purpose |
|-------|---------|---------|
| **pkg_1–9** | `cargo test --test pkg_*` | Chain-only, contract-focused regression |
| **suite_a–z** | `cargo test --test suite_*` | Cross-service flows (facilitator, relayer, MCP, MPP) |
| **session** | `suite_session_simulator` | Simulator session helpers |

Default `./run_all_tests.sh` runs suites only; use `--pkg` or `--all` for package tests.

### TestEnv harness

Use `TestEnv` instead of hand-rolling simulator startup:

```rust
use crate::common::TestEnv;

let env = TestEnv::chain_only().await;
let (env, validation_addr, _) = TestEnv::with_validation_agent().await;
```

## Known limitations

- Default `./run_all_tests.sh` skips `pkg_*` to avoid duplicating flows already covered by integration suites. Use `--pkg` for contract-focused regression.

## License

MIT
