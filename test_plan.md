# Agentic Commerce Test Plan & Design

**Objective:** Detailed design for a "Chain Simulator" based test suite that orchestrates the entire MultiversX Agentic Commerce ecosystem without mocks.

## 1. Architecture: The "Orchestrator" Pattern

We will build a Rust-based orchestration suite that manages the lifecycle of all components.

### 1.1. Components (Real Instances)
1.  **Chain Simulator**: `mx-chain-simulator-go` (running on port 8085).
2.  **Smart Contract**: `mx-8004` (deployed on Simulator).
3.  **MCP Server**: `multiversx-mcp-server` (running as a Node.js process, port 3001).
4.  **Facilitator**: `x402-facilitator` (running as a Node.js process, port 3000).
5.  **Agent (Moltbot)**: `moltbot-starter-kit` (Node.js process).
6.  **Skills Bundle**: `multiversx-openclaw-skills` (Loaded by Moltbot).

### 1.2. Network Topology
```mermaid
graph TD
    TestRunner[Rust Test Runner] -->|Control| ChainSim[mx-chain-simulator-go]
    TestRunner -->|Control| MCPServer
    TestRunner -->|Control| Facilitator
    TestRunner -->|Control| MoltbotA
    TestRunner -->|Control| MoltbotB
    
    MCPServer -->|HTTP Proxy| ChainSim
    Facilitator -->|HTTP Proxy| ChainSim
    MoltbotA -->|HTTP Proxy| ChainSim
    MoltbotB -->|HTTP Proxy| ChainSim
    
    MoltbotA -->|Finds| MCPServer
    MoltbotA -->|Pays| Facilitator
    MoltbotB -->|Pays| Facilitator
```

---

## 2. Test Suites

### Suite A: Moltbot Lifecycle (Happy Paths)
*Focus: The journey of a single agent from birth to getting paid.*

| ID | Scenario | Steps | Expected Result |
| :--- | :--- | :--- | :--- |
| **ML-001** | **Registration & Discovery** | 1. Moltbot starts up.<br>2. Generates wallet (`wallet.pem`).<br>3. Calls `scripts/register.ts`.<br>4. MCP Server indexes `registerAgent` tx.<br>5. Test Runner queries MCP `get-agent-manifest`. | Agent is discoverable on MCP with correct Price/Token. |
| **ML-002** | **The "Get Paid" Flow (Passive)** | 1. Test Runner (Client) sends HTTP GET to Moltbot.<br>2. Moltbot returns 402 with Facilitator URL.<br>3. Client calls Facilitator `/verify` -> `/settle`.<br>4. Facilitator calls ChainSim.<br>5. Moltbot receives WebSocket event `payment_verified`.<br>6. Moltbot executes task. | Task completed, response returned to Client. |
| **ML-003** | **Reputation Loop** | 1. After ML-002, Moltbot calls `multiversx:prove` (Validation Registry).<br>2. Oracle (Test Runner) verifies job on-chain.<br>3. Client rates job via Reputation Registry. | Moltbot score increases. |

### Suite B: Multi-Agent Coordination (Happy Paths)
*Focus: Agents hiring Agents.*

| ID | Scenario | Steps | Expected Result |
| :--- | :--- | :--- | :--- |
| **MA-001** | **Task Delegation** | 1. **Agent A** (Buyer) receives a complex task.<br>2. Agent A queries MCP: "Who can do X?".<br>3. MCP returns **Agent B** (Seller).<br>4. Agent A hits Agent B's endpoint -> gets 402.<br>5. Agent A uses `multiversx:pay` skill.<br>6. Agent B works -> proves.<br>7. Agent A receives result. | A pays B using on-chain funds.<br>B delivers result to A.<br>B gets paid. |
| **MA-002** | **Chain of Command** | Agent A hires B, B hires C. | A pays B, B pays C. All proofs linked on-chain. |

### Suite C: Edge Cases & Failure Modes
*Focus: Robustness under pressure.*

| ID | Scenario | Condition | Expected Result |
| :--- | :--- | :--- | :--- |
| **EC-001** | **Insufficient Allowance** | Agent A tries to pay Agent B but has 0 USDC. | `multiversx:pay` fails gracefully with "Insufficient Funds". |
| **EC-002** | **Service Down** | Agent A tries to pay, but Facilitator is offline. | `multiversx:pay` retries or fails with "Payment Gateway Unavailable". |
| **EC-003** | **Double Spend Attempt** | Agent A tries to use the same Payment Nonce twice. | Facilitator rejects 2nd request. |
| **EC-004** | **Unverified Job Rating** | Client tries to rate Agent before `verifyJob` tx is mined. | Reputation Contract reverts. |
| **EC-005** | **Expired Offer** | Client tries to pay an Invoice that is > 1 hour old. | Facilitator rejects (Timestamp check). |

### Suite D: MCP Server Features (Comprehensive)
*Focus: Full coverage of all MCP tools and resources.*

| ID | Scenario | Tool | Expected Result |
| :--- | :--- | :--- | :--- |
| **MCP-001** | **Get Balance** | `get-balance` | EGLD balance matches ChainSim state. |
| **MCP-002** | **Query Account** | `query-account` | Nonce, Balance, CodeHash match ChainSim. |
| **MCP-003** | **Send EGLD** | `send-egld` | TxHash returned, Receiver balance increases. |
| **MCP-004** | **Send EGLD Bulk** | `send-egld-multiple` | Multiple receivers updated correctly. |
| **MCP-005** | **Issue Fungible** | `issue-fungible` | Token ID returned (e.g. `TEST-123456`). |
| **MCP-006** | **Issue SFT/NFT** | `issue-nft-collection` | Collection ID returned. |
| **MCP-007** | **Create NFT** | `create-nft` | NFT Nonce returned. |
| **MCP-008** | **Send Tokens** | `send-tokens` | ESDT/NFT transfer successful. |
| **MCP-009** | **Send Tokens Bulk** | `send-tokens-multiple` | Bulk ESDT transfer successful. |
| **MCP-010** | **Create Relayed V3** | `create-relayed-v3` | Returns a signed tx with `Relayer` field set. |
| **MCP-011** | **Track Transaction** | `track-transaction` | Polls until status is `success`. |
| **MCP-012** | **Search Products** | `search-products` | Returns mocked/indexed product data. |
| **MCP-013** | **Get Agent Pricing** | `get-agent-pricing` | Returns Price/Token from Registry for given Nonce. |

---

## 3. Implementation Plan Update

### New Tasks
1.  **Integrate `multiversx-openclaw-skills`**: Ensure the test runner can "inject" this bundle into the Moltbot instance (or point Moltbot to it).
2.  **Multi-Instance Support**: Update `ProcessManager` to support spawning `moltbot_a`, `moltbot_b` with different configurations (Ports, Wallets).
3.  **Client Simulator**: Implement a Rust HTTP client that mimics a user hitting the 402 endpoint, parsing the header, and talking to the Facilitator.
4.  **MCP Feature Tests**: Implement `suite_g_mcp_features.rs` to iterate through all MCP-0xx scenarios.

## 4. Verification
- `./run_all_tests.sh` — default tier (MPP unit + suites A–T/Z/session + off-chain U–Y)
- `./run_all_tests.sh --pkg` — chain-only package tests (`pkg_1`–`pkg_9`)
- `cargo itest --test pkg_1_identity` — smoke tier (single package)
- `npm run test:unit` / `RUN_CHAIN_TESTS=1 npm run test:chain` — MPP TypeScript

---

## 5. Current Test Map (2026)

### Rust packages (`tests/pkg_*`)

| Target | Focus |
|--------|--------|
| `pkg_1_identity` | Agent registration, metadata, tokens, service configs |
| `pkg_2_validation` | Job lifecycle, payments, `clean_old_jobs` (incl. boundary) |
| `pkg_3_reputation` | Feedback, averaging, direct feedback |
| `pkg_4_facilitator` | EGLD/ESDT verify & settle, idempotency, relayed v3 |
| `pkg_5_mcp` | MCP balance, registry, transfer tools |
| `pkg_6_moltbot` | Registration and gasless flows |
| `pkg_7_e2e` | Gasless end-to-end |
| `pkg_8_escrow` | Deposit, release, refund, multi-job |
| `pkg_9_escrow_lifecycle` | Cascading escrow |

### Rust suites (`tests/suite_*`)

| Target | Focus |
|--------|--------|
| `suite_a`–`d` | Identity, validation, reputation, facilitator smoke |
| `suite_e`–`k` | Moltbot lifecycle, relayed ops, facilitator settle |
| `suite_l`–`t` | MCP discovery, agent-to-agent, extended MCP |
| `suite_u`–`y` | Facilitator/relayer advanced, E2E flows |
| `suite_z` | MPP + facilitator integration |
| `suite_session_extended` | MPP session top-up, slashing, negative flows (chain-sim) |
| `suite_aa` | MPP facilitator session/subscription 402 resources |
| `suite_ec` | Facilitator offline (EC-002) |
| `suite_v3` | Relayer quota HTTP 429 |

### TypeScript

| File | When to run |
|------|-------------|
| `tests/mpp-402.test.ts` | Always (402 challenge, no chain) |
| `tests/mpp-payment.test.ts` | `RUN_CHAIN_TESTS=1` + funded test keys |

### Shared harness (`tests/common/`)

- `simulator.rs` — readiness polling, block generation, time advance
- `test_env.rs` — `TestEnv::chain_only()`, `with_registries()`, etc.
- `services.rs` — `start_facilitator()` on dynamic port
- PEM files use `create_temp_pem_file()` (system temp dir, not repo root)

### CI tiers

| Workflow | Scope |
|----------|--------|
| `test-smoke.yml` | MPP unit + `pkg_1_identity` + `suite_d_facilitator` |
| `test-nightly.yml` | Full `run_all_tests.sh` with `FAIL_ON_RETRY=1` |
