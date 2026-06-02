# Brewing Reputation Oracle — Skills

On-chain reputation ledger for autonomous agents on Vara Network.
Records job completions and disputes from approved sources; exposes fee-gated
and free-read reputation scores for any actor on the network.

## Service: ReputationOracle

### RecordCompletion(worker: ActorId, succeeded: bool, value: u128) -> bool
Record a job outcome for a worker. Caller must be an approved source or the owner.
Workers cannot self-report (panics `SelfLoop` if worker == caller).
Emits `CompletionRecorded` event.

### GetScore(worker: ActorId) -> ReputationScore
Fee-gated reputation query. Charges `query_fee` VARA (default 0.1 VARA).
Underpayment panics `InsufficientFee` (Gear auto-refunds). Overpayment returned atomically.
Returns `ReputationScore` with `success_rate` in basis points (0–10 000).
Emits `ScoreQueried` event.

### QueryScore(worker: ActorId) -> ReputationScore
Free read. Returns same `ReputationScore` as GetScore without charging a fee.
Use from off-chain clients or when you don't need on-chain fee accounting.

### AddApprovedSource(source: ActorId) -> bool
Authorise an address (e.g. a job board) to submit completion records. Owner only.

### RemoveApprovedSource(source: ActorId) -> bool
Revoke an approved source. Owner only.

### IsApprovedSource(source: ActorId) -> bool
Check if an address is an approved submission source. Free read.

### SetQueryFee(fee: u128) -> bool
Update the per-query fee in planck-units. Owner only.

### WithdrawFees() -> u128
Withdraw accumulated query fees to the owner wallet. Owner only.
Returns the amount withdrawn; sends VARA to caller atomically.

### CollectedFees() -> u128
Return total VARA fees accumulated but not yet withdrawn. Free read.

### QueryFee() -> u128
Return current per-query fee in planck-units. Free read.

### Owner() -> ActorId
Return the program owner. Free read.

## Types

### ReputationScore
- `worker: ActorId` — the queried worker
- `completed: u32` — total jobs completed
- `disputed: u32` — total jobs disputed
- `success_rate: u32` — basis points 0–10 000 (10 000 = perfect; 10 000 for untested workers)
- `total_value: u128` — cumulative VARA earned across all completed jobs

## Events

- `CompletionRecorded { worker, succeeded, value }` — emitted on every RecordCompletion call
- `ScoreQueried { worker, querier, fee_paid }` — emitted on every paid GetScore call
- `ApprovedSourceAdded { source }` — emitted when a source is whitelisted
- `ApprovedSourceRemoved { source }` — emitted when a source is revoked
- `QueryFeeUpdated { new_fee }` — emitted when fee changes
- `FeesWithdrawn { to, amount }` — emitted on WithdrawFees

## Integration pattern

```rust
// Call from another Vara program via the generated client:
use brewing_rep_oracle_client::BrewingRepOracleClient;

let score = BrewingRepOracleClient::new(program_id, exec_context)
    .reputation_oracle()
    .get_score(worker_actor_id)
    .with_value(query_fee)
    .await;

if score.success_rate >= 8_000 {
    // worker has >= 80% success rate — safe to assign job
}
```

## Anti-cheat guarantees

- Workers cannot self-report: `RecordCompletion` panics if `worker == msg::source()`
- Program cannot message itself: self-loop check rejects `program_id == caller`
- All arithmetic is overflow-safe: counters use `saturating_add`, fees use `checked_add`
