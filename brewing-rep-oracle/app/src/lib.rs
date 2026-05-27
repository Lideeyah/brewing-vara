#![no_std]

use core::cell::RefCell;
use sails_rs::collections::{BTreeMap, BTreeSet};
use sails_rs::gstd::{exec, msg};
use sails_rs::prelude::*;

/// Default query fee: 0.1 VARA (100_000_000_000 planck-units, 12 decimals)
pub const DEFAULT_QUERY_FEE: u128 = 100_000_000_000;

// ─── Public types ────────────────────────────────────────────────────────────

/// Per-worker reputation entry stored in the on-chain ledger.
#[sails_rs::sails_type]
#[derive(Clone, Debug, PartialEq, Default)]
pub struct ReputationEntry {
    pub completed: u32,
    pub disputed: u32,
    /// Cumulative VARA (planck-units) from all completed jobs.
    pub total_value: u128,
    /// Block timestamp (ms) of the last update.
    pub last_updated: u64,
}

/// Reputation score returned to callers of GetScore / QueryScore.
#[sails_rs::sails_type]
#[derive(Clone, Debug, PartialEq, Default)]
pub struct ReputationScore {
    pub worker: ActorId,
    pub completed: u32,
    pub disputed: u32,
    /// Basis points (0-10 000). 10 000 if the worker has no history.
    pub success_rate: u32,
    pub total_value: u128,
}

/// Typed errors returned by the oracle service.
#[sails_rs::sails_type]
#[derive(Clone, Debug, PartialEq)]
pub enum OracleError {
    /// msg::value() < query_fee. Attached value is refunded via CommandReply.
    InsufficientFee,
    /// Caller is not the owner and not in approved_sources.
    NotOwner,
    /// A program tried to call itself (leaderboard anti-cheat).
    SelfLoop,
    /// Arithmetic overflow in fee accounting.
    ArithmeticOverflow,
}

// ─── Internal helpers ────────────────────────────────────────────────────────

fn compute_score(worker: ActorId, entry: &ReputationEntry) -> ReputationScore {
    let total = entry.completed.saturating_add(entry.disputed);
    let success_rate = if total == 0 {
        10_000 // perfect score for untested workers
    } else {
        (entry.completed as u128 * 10_000 / total as u128) as u32
    };
    ReputationScore {
        worker,
        completed: entry.completed,
        disputed: entry.disputed,
        success_rate,
        total_value: entry.total_value,
    }
}

// ─── Program state ───────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct BrewingRepOracleState {
    pub owner: ActorId,
    pub query_fee: u128,
    pub collected_fees: u128,
    pub ledger: BTreeMap<ActorId, ReputationEntry>,
    pub approved_sources: BTreeSet<ActorId>,
}

impl Default for BrewingRepOracleState {
    fn default() -> Self {
        Self {
            owner: ActorId::zero(),
            query_fee: DEFAULT_QUERY_FEE,
            collected_fees: 0,
            ledger: BTreeMap::new(),
            approved_sources: BTreeSet::new(),
        }
    }
}

// ─── Events ──────────────────────────────────────────────────────────────────
//
// NOTE: variants MUST stay in case-insensitive alphabetical order so that the
// macro-generated INTERFACE_ID (Rust-declaration order) matches the IDL-AST
// recomputed ID (post-normalize alphabetical order).  Do NOT reorder.

#[sails_rs::sails_type]
#[sails_rs::event]
pub enum ReputationOracleEvent {
    ApprovedSourceAdded {
        source: ActorId,
    },
    ApprovedSourceRemoved {
        source: ActorId,
    },
    CompletionRecorded {
        worker: ActorId,
        succeeded: bool,
        value: u128,
    },
    FeesWithdrawn {
        to: ActorId,
        amount: u128,
    },
    QueryFeeUpdated {
        new_fee: u128,
    },
    ScoreQueried {
        worker: ActorId,
        querier: ActorId,
        fee_paid: u128,
    },
}

// ─── Service ─────────────────────────────────────────────────────────────────

pub struct ReputationOracle<
    S: StateMut<Item = BrewingRepOracleState, Error = Infallible> =
        RefCell<BrewingRepOracleState>,
> {
    state: S,
}

impl<S: StateMut<Item = BrewingRepOracleState, Error = Infallible>> ReputationOracle<S> {
    pub fn new(state: S) -> Self {
        Self { state }
    }
}

// NOTE: methods MUST stay in case-insensitive alphabetical order by their
// PascalCase IDL name so the macro hash (Rust order) = IDL-AST hash (sorted).
// Alphabetical: AddApprovedSource, CollectedFees, GetScore, IsApprovedSource,
//               Owner, QueryFee, QueryScore, RecordCompletion,
//               RemoveApprovedSource, SetQueryFee, WithdrawFees.
// Do NOT reorder.

#[sails_rs::service(events = ReputationOracleEvent)]
impl<S: StateMut<Item = BrewingRepOracleState, Error = Infallible>> ReputationOracle<S> {
    // ─── AddApprovedSource ───────────────────────────────────────────────────

    /// Add an approved source (e.g., a job board). Owner only.
    #[export]
    pub fn add_approved_source(&mut self, source: ActorId) -> bool {
        let caller = Syscall::message_source();
        if caller != self.state.get().owner {
            panic!("NotOwner");
        }
        self.state.get_mut().approved_sources.insert(source);
        self.emit_event(ReputationOracleEvent::ApprovedSourceAdded { source })
            .unwrap();
        true
    }

    // ─── CollectedFees ───────────────────────────────────────────────────────

    /// Return total VARA fees collected but not yet withdrawn.
    #[export]
    pub fn collected_fees(&self) -> u128 {
        self.state.get().collected_fees
    }

    // ─── GetScore ────────────────────────────────────────────────────────────

    /// Query a worker's reputation score. Charges `query_fee` VARA.
    ///
    /// - Underpayment: panics `"InsufficientFee"` — Gear auto-refunds attached value on panic.
    /// - Overpayment: excess returned atomically via `CommandReply::with_value`.
    /// - Exact payment: no refund needed.
    #[export]
    pub fn get_score(&mut self, worker: ActorId) -> CommandReply<ReputationScore> {
        let paid = msg::value();
        let query_fee = self.state.get().query_fee;

        // Leaderboard anti-cheat: reject self-loop calls (panic = auto-refund)
        if Syscall::message_source() == Syscall::program_id() {
            panic!("SelfLoop");
        }

        // 1. Value guard (panic = auto-refund on underpayment)
        if paid < query_fee {
            panic!("InsufficientFee");
        }

        // 2. Overflow-checked fee accounting
        let mut state = self.state.get_mut();
        let new_collected = state
            .collected_fees
            .checked_add(query_fee)
            .expect("ArithmeticOverflow");
        state.collected_fees = new_collected;

        // 3. Compute score
        let score = {
            let entry = state.ledger.get(&worker).cloned().unwrap_or_default();
            compute_score(worker, &entry)
        };
        drop(state);

        let querier = Syscall::message_source();
        self.emit_event(ReputationOracleEvent::ScoreQueried {
            worker,
            querier,
            fee_paid: query_fee,
        })
        .unwrap();

        // 4. Refund excess atomically (CommandReply carries value back)
        let excess = paid - query_fee;
        CommandReply::new(score).with_value(excess)
    }

    // ─── IsApprovedSource ────────────────────────────────────────────────────

    /// Check if an address is an approved submission source.
    #[export]
    pub fn is_approved_source(&self, source: ActorId) -> bool {
        self.state.get().approved_sources.contains(&source)
    }

    // ─── Owner ───────────────────────────────────────────────────────────────

    /// Return the program owner.
    #[export]
    pub fn owner(&self) -> ActorId {
        self.state.get().owner
    }

    // ─── QueryFee ────────────────────────────────────────────────────────────

    /// Return the current per-query fee in planck-units.
    #[export]
    pub fn query_fee(&self) -> u128 {
        self.state.get().query_fee
    }

    // ─── QueryScore ──────────────────────────────────────────────────────────

    /// Return a worker's reputation score — free read, no fee charged.
    #[export]
    pub fn query_score(&self, worker: ActorId) -> ReputationScore {
        let state = self.state.get();
        let entry = state.ledger.get(&worker).cloned().unwrap_or_default();
        compute_score(worker, &entry)
    }

    // ─── RecordCompletion ────────────────────────────────────────────────────

    /// Record a job completion (or dispute) for a worker.
    ///
    /// Caller must be an approved source OR the owner.
    /// Workers cannot self-report (worker != msg::source()).
    #[export]
    pub fn record_completion(
        &mut self,
        worker: ActorId,
        succeeded: bool,
        value: u128,
    ) -> bool {
        let caller = Syscall::message_source();

        // Domain anti-cheat: workers can't self-report
        if worker == caller {
            panic!("SelfLoop");
        }

        // Auth guard
        {
            let state = self.state.get();
            if caller != state.owner && !state.approved_sources.contains(&caller) {
                panic!("NotOwner");
            }
        }

        // Update ledger with overflow-checked arithmetic
        {
            let mut state = self.state.get_mut();
            let entry = state.ledger.entry(worker).or_default();
            if succeeded {
                entry.completed = entry.completed.saturating_add(1);
                entry.total_value = entry.total_value.saturating_add(value);
            } else {
                entry.disputed = entry.disputed.saturating_add(1);
            }
            entry.last_updated = exec::block_timestamp();
        }

        self.emit_event(ReputationOracleEvent::CompletionRecorded {
            worker,
            succeeded,
            value,
        })
        .unwrap();
        true
    }

    // ─── RemoveApprovedSource ────────────────────────────────────────────────

    /// Remove an approved source. Owner only.
    #[export]
    pub fn remove_approved_source(&mut self, source: ActorId) -> bool {
        let caller = Syscall::message_source();
        if caller != self.state.get().owner {
            panic!("NotOwner");
        }
        self.state.get_mut().approved_sources.remove(&source);
        self.emit_event(ReputationOracleEvent::ApprovedSourceRemoved { source })
            .unwrap();
        true
    }

    // ─── SetQueryFee ─────────────────────────────────────────────────────────

    /// Update the per-query fee. Owner only.
    #[export]
    pub fn set_query_fee(&mut self, fee: u128) -> bool {
        let caller = Syscall::message_source();
        if caller != self.state.get().owner {
            panic!("NotOwner");
        }
        self.state.get_mut().query_fee = fee;
        self.emit_event(ReputationOracleEvent::QueryFeeUpdated { new_fee: fee })
            .unwrap();
        true
    }

    // ─── WithdrawFees ────────────────────────────────────────────────────────

    /// Withdraw accumulated fees to the owner wallet. Owner only.
    /// Panics `"NotOwner"` if caller is not the owner.
    #[export]
    pub fn withdraw_fees(&mut self) -> CommandReply<u128> {
        let caller = Syscall::message_source();
        if caller != self.state.get().owner {
            panic!("NotOwner");
        }
        let amount = self.state.get().collected_fees;
        self.state.get_mut().collected_fees = 0;
        self.emit_event(ReputationOracleEvent::FeesWithdrawn { to: caller, amount })
            .unwrap();
        CommandReply::new(amount).with_value(amount)
    }
}

// ─── Root program ─────────────────────────────────────────────────────────────

pub struct Program {
    state: RefCell<BrewingRepOracleState>,
}

#[sails_rs::program]
impl Program {
    /// Constructor.
    ///
    /// - `owner`: defaults to `msg::source()` (the deploying wallet).
    /// - `query_fee`: defaults to `DEFAULT_QUERY_FEE` (0.1 VARA).
    pub fn init(owner: Option<ActorId>, query_fee: Option<u128>) -> Self {
        let effective_owner = owner.unwrap_or_else(|| Syscall::message_source());
        let effective_fee = query_fee.unwrap_or(DEFAULT_QUERY_FEE);
        Self {
            state: RefCell::new(BrewingRepOracleState {
                owner: effective_owner,
                query_fee: effective_fee,
                collected_fees: 0,
                ledger: BTreeMap::new(),
                approved_sources: BTreeSet::new(),
            }),
        }
    }

    /// Expose the ReputationOracle service.
    pub fn reputation_oracle(&self) -> ReputationOracle<&RefCell<BrewingRepOracleState>> {
        ReputationOracle::new(&self.state)
    }
}
