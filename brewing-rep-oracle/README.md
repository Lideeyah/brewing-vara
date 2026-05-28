# Brewing Reputation Oracle

**The trust primitive for autonomous agent economies on Vara.**

Every time an AI agent completes a job, gets paid, or fails a task — this program records it on-chain. Any contract can query a worker's track record before releasing funds. No centralized registry. No self-reporting. No trust required.

---

## The Problem

Autonomous agents can't vouch for themselves. In a world where AI agents hire other AI agents — how does an employer know the worker is reliable before locking USDC in escrow?

Off-chain reputation is gameable. On-chain reputation, written only by verified job boards and settled by the ledger itself, is not.

---

## What It Is

`brewing-rep-oracle` is a Vara Sails program that maintains a **tamper-proof, composable reputation ledger** for any actor on the network.

- Approved job boards (not workers) submit completion records
- Workers cannot self-report — the program rejects any call where `worker == msg::source()`
- Reputation is expressed as a **success rate in basis points** (0–10,000), queryable free or fee-gated
- Any Vara program can call `RecordCompletion` to write, or `QueryScore` / `GetScore` to read
- Accumulated query fees are withdrawable by the owner — creating a sustainable oracle fee model

This is infrastructure. It's the primitive that makes trustless agent coordination possible on Vara.

---

## Architecture

```
Job Board / Escrow Program
        │
        │ RecordCompletion(worker, succeeded, value)
        ▼
 ┌──────────────────────────────┐
 │   BrewingRepOracle (Vara)    │
 │                              │
 │  ledger: ActorId → Entry     │
 │    .completed: u32           │
 │    .disputed:  u32           │
 │    .total_value: u128 (VARA) │
 │    .last_updated: u64 (ms)   │
 │                              │
 │  approved_sources: BTreeSet  │
 └──────────────────────────────┘
        │
        │ QueryScore(worker) → ReputationScore
        │   .success_rate: u32  (basis points)
        │   .completed / .disputed / .total_value
        ▼
 Employer Program / Off-chain Client
```

---

## Interface

Full IDL at [`brewing_rep_oracle.idl`](brewing_rep_oracle.idl).

### Write functions

| Function | Auth | Description |
|---|---|---|
| `RecordCompletion(worker, succeeded, value)` | Approved source or owner | Write a job outcome. Workers cannot self-report. |
| `AddApprovedSource(source)` | Owner | Authorise a job board to submit records. |
| `RemoveApprovedSource(source)` | Owner | Revoke a source. |
| `SetQueryFee(fee)` | Owner | Update per-query fee in planck-units. |
| `WithdrawFees()` | Owner | Pull accumulated query fees to owner wallet. |

### Read functions

| Function | Cost | Description |
|---|---|---|
| `QueryScore(worker)` | Free | Off-chain reputation lookup — no fee. |
| `GetScore(worker)` | 0.1 VARA | On-chain composable query — charges `query_fee`. Overpayment returned atomically. |
| `IsApprovedSource(source)` | Free | Check source authorisation status. |
| `CollectedFees()` | Free | Total VARA in fee balance. |
| `QueryFee()` | Free | Current per-query fee. |
| `Owner()` | Free | Contract owner address. |

### Events

All state changes emit typed events: `CompletionRecorded`, `ApprovedSourceAdded`, `ApprovedSourceRemoved`, `ScoreQueried`, `QueryFeeUpdated`, `FeesWithdrawn`.

---

## Key Properties

**Anti-cheat.** Two self-loop checks:
1. Workers cannot call `RecordCompletion` for themselves (`worker == caller` → panic, auto-refund)
2. The program cannot message itself (`program_id == caller` → panic, auto-refund)

**Overflow-safe.** All counters use `saturating_add`. Fee accounting uses `checked_add` with explicit panic on overflow.

**Composable.** `GetScore` is designed for on-chain callers. Underpayment panics (Gear auto-refunds). Overpayment returns excess via `CommandReply::with_value`. Exact payment: no refund overhead.

**Untested workers start at 10,000 bps.** A fresh worker's success rate is 100% — neutral default, not punished for having no history.

---

## Build

```bash
# Install Vara/Gear toolchain
rustup target add wasm32-unknown-unknown

# Build the WASM binary and generate IDL
cargo build --release
```

Output: `target/wasm32-unknown-unknown/release/brewing_rep_oracle.opt.wasm`

---

## Test

```bash
cargo test --release
```

The test suite covers:
- Owner defaults to deployer
- Auth guards on all write functions (stranger panics)
- `RecordCompletion` updates ledger correctly
- Success rate calculation (basis points)
- `QueryScore` (free) matches `GetScore` (fee-gated) output
- `WithdrawFees` transfers collected balance
- `SetQueryFee` updates fee
- Event emission verified with stream listener

---

## Deploy to Vara Testnet

1. Get VARA testnet tokens from the [Vara faucet](https://idea.gear-tech.io)
2. Open [idea.gear-tech.io](https://idea.gear-tech.io) → Upload Program
3. Upload `brewing_rep_oracle.opt.wasm` with `brewing_rep_oracle.idl`
4. Call `Init(None, None)` — owner = your wallet, fee = 0.1 VARA default
5. Copy the program address and call `AddApprovedSource` with your job board's address

---

## Why Vara

Vara's actor model makes this program composable by design. Any program can message `BrewingRepOracle` directly — no bridges, no APIs, no trusted intermediaries. The same program that locks USDC in escrow can query reputation before releasing funds, in a single asynchronous message chain. That's not possible on most chains.

Sails gives the IDL as the source of truth. Client code is generated from it. Any program that imports the client can call this oracle as if it were a typed Rust function.

---

## License

MIT
