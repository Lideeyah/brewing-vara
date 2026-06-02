<div align="center">

# ◈ Brewing Reputation Oracle

### The trust primitive for autonomous agent economies on Vara.

*Before you pay an agent — know if it's ever been trusted.*

[![License: MIT](https://img.shields.io/badge/License-MIT-green.svg)](LICENSE)
[![Network](https://img.shields.io/badge/Vara-Testnet-brightgreen)](https://idea.gear-tech.io/programs/0x0971390d00f050f1429fe82b13dbed0d2c37e86ea06a0bfdc516029bf811e3a1?node=wss://testnet.vara.network)
[![Built with Sails](https://img.shields.io/badge/Built%20with-Sails-blue)](https://github.com/gear-tech/sails)
[![Tests](https://img.shields.io/badge/Tests-Passing-success)](#test)

**[View on Vara Explorer →](https://idea.gear-tech.io/programs/0x0971390d00f050f1429fe82b13dbed0d2c37e86ea06a0bfdc516029bf811e3a1?node=wss://testnet.vara.network)**

</div>

---

## The Problem

Autonomous agents are hiring other autonomous agents. Right now, there is no way to know if the agent you're about to pay has ever completed a job, failed one, or been disputed.

Off-chain reputation is gameable. Self-reported reputation is worthless. And most chains can't compose trust checks inside smart contract logic.

**Vara can. This program does.**

---

## What It Does

`BrewingRepOracle` is an on-chain reputation ledger deployed on Vara. It tracks every job completion and dispute for any actor — and exposes that history as a composable, fee-gated query any Vara program can call.

```rust
// Inside your escrow or job board program:
// Check reputation before releasing funds — one call, on-chain.

let score = BrewingRepOracleClient::new(oracle_program_id, exec_context)
    .reputation_oracle()
    .get_score(worker_actor_id)
    .with_value(query_fee)   // 0.1 VARA
    .await;

if score.success_rate < 8_000 {
    // Below 80% success rate — reject or require higher collateral
    return Err("Worker reputation insufficient");
}
// Proceed with settlement
```

No API call. No bridge. No trusted intermediary. Pure on-chain composability.

---

## Deployed Program

| Network | Address |
|---|---|
| **Vara Testnet** | `0x0971390d00f050f1429fe82b13dbed0d2c37e86ea06a0bfdc516029bf811e3a1` |

---

## How It Works

```
  Job Board / Escrow Contract
           │
           │  RecordCompletion(worker, succeeded, value)
           │  ← Only approved sources. Workers cannot self-report.
           ▼
  ┌─────────────────────────────────────┐
  │        BrewingRepOracle             │
  │                                     │
  │  ledger: ActorId → {                │
  │    completed:    u32,               │
  │    disputed:     u32,               │
  │    total_value:  u128,  (VARA)      │
  │    last_updated: u64,   (ms)        │
  │  }                                  │
  │                                     │
  │  approved_sources: BTreeSet<ActorId>│
  └─────────────────────────────────────┘
           │
           │  GetScore(worker) → ReputationScore
           │    .success_rate  — basis points 0–10,000
           │    .completed     — total jobs done
           │    .disputed      — total disputes
           │    .total_value   — lifetime VARA earned
           ▼
  Employer Program / Off-chain Dashboard
```

**Three guarantees baked into the contract:**

| Guarantee | How |
|---|---|
| Workers can't lie | `worker == msg::source()` → panic + auto-refund |
| No overflow | `saturating_add` on counters, `checked_add` on fees |
| Composable on-chain | `GetScore` returns excess VARA atomically via `CommandReply::with_value` |

---

## Interface

Full IDL: [`brewing_rep_oracle.idl`](brewing_rep_oracle.idl) · Skills: [`skills.md`](skills.md)

### Write — approved sources only

| Method | Who | What |
|---|---|---|
| `RecordCompletion(worker, succeeded, value)` | Approved source / owner | Record a job outcome |
| `AddApprovedSource(source)` | Owner | Whitelist a job board |
| `RemoveApprovedSource(source)` | Owner | Revoke a source |
| `SetQueryFee(fee)` | Owner | Change per-query fee |
| `WithdrawFees()` | Owner | Collect accumulated VARA fees |

### Read — open to anyone

| Method | Cost | Returns |
|---|---|---|
| `QueryScore(worker)` | **Free** | `ReputationScore` — for off-chain clients |
| `GetScore(worker)` | **0.1 VARA** | `ReputationScore` — composable on-chain query |
| `IsApprovedSource(source)` | Free | `bool` |
| `CollectedFees()` | Free | `u128` |

### Events

`CompletionRecorded` · `ScoreQueried` · `ApprovedSourceAdded` · `ApprovedSourceRemoved` · `QueryFeeUpdated` · `FeesWithdrawn`

---

## Build & Test

```bash
# Build WASM + generate IDL
cargo build --release

# Run full test suite
cargo test --release
```

**Test coverage:**
- Auth guards: owner-only and approved-source-only writes enforced
- Anti-cheat: self-report rejected, self-loop rejected
- Ledger: completion records update correctly
- Math: basis-point success rate calculation
- Parity: `QueryScore` (free) matches `GetScore` (paid) output
- Events: stream listener verifies emission on every state change
- Fee lifecycle: `SetQueryFee` → `GetScore` → `WithdrawFees` end-to-end

---

## Why Vara

Every program on Vara can message every other program directly — asynchronously, with typed arguments, no bridges. `GetScore` was designed for this: an escrow contract can check reputation and release funds in the same message chain that receives job completion.

Sails generates the client crate from the IDL. Any program importing `brewing-rep-oracle-client` calls this oracle like a typed Rust function. The reputation layer composes with any Vara protocol — job boards, prediction markets, agent orchestrators, staking programs — without any changes to this oracle.

---

## License

MIT — build on it.
