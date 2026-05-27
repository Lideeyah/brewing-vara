use ::brewing_rep_oracle_app::DEFAULT_QUERY_FEE;
use ::brewing_rep_oracle_client::{
    BrewingRepOracleClient as _,
    BrewingRepOracleClientCtors as _,
    BrewingRepOracleClientProgram,
    reputation_oracle::{ReputationOracle as _, ReputationScore, events},
};
use sails_rs::prelude::futures::StreamExt as _;
#[allow(unused_imports)]
use sails_rs::{client::*, gtest::*, prelude::*};

// ─── Test wallets ─────────────────────────────────────────────────────────────

const OWNER: u64 = DEFAULT_USER_ALICE;
const APPROVED_SOURCE: u64 = DEFAULT_USER_BOB;
const WORKER: u64 = 300;
const STRANGER: u64 = 400;

/// Deploy the program with OWNER as the deploying account.
async fn deploy(env: &GtestEnv) -> Actor<BrewingRepOracleClientProgram, GtestEnv> {
    let code_id = env.system().submit_code(::brewing_rep_oracle::WASM_BINARY);
    env.deploy::<BrewingRepOracleClientProgram>(code_id, b"salt".to_vec())
        .init(None, None) // owner = OWNER, fee = DEFAULT_QUERY_FEE
        .with_actor_id(OWNER.into())
        .await
        .unwrap()
}

// ─── Auth / access control ────────────────────────────────────────────────────

#[tokio::test]
async fn owner_is_set_to_deployer_by_default() {
    let env = GtestEnv::system_default();
    let program = deploy(&env).await;
    let svc = program.reputation_oracle();

    let owner: ActorId = svc.owner().await.unwrap();
    assert_eq!(ActorId::from(OWNER), owner);
}

#[tokio::test]
async fn add_approved_source_by_owner_succeeds() {
    let env = GtestEnv::system_default();
    let program = deploy(&env).await;
    let mut svc = program.reputation_oracle();

    let ok = svc
        .add_approved_source(ActorId::from(APPROVED_SOURCE))
        .with_actor_id(OWNER.into())
        .await
        .unwrap();
    assert!(ok);
    assert!(svc
        .is_approved_source(ActorId::from(APPROVED_SOURCE))
        .await
        .unwrap());
}

#[tokio::test]
#[should_panic]
async fn add_approved_source_by_stranger_panics() {
    let env = GtestEnv::system_default();
    let program = deploy(&env).await;
    let mut svc = program.reputation_oracle();

    svc.add_approved_source(ActorId::from(APPROVED_SOURCE))
        .with_actor_id(STRANGER.into())
        .await
        .unwrap();
}

#[tokio::test]
async fn remove_approved_source_works() {
    let env = GtestEnv::system_default();
    let program = deploy(&env).await;
    let mut svc = program.reputation_oracle();

    // Add then remove
    svc.add_approved_source(ActorId::from(APPROVED_SOURCE))
        .with_actor_id(OWNER.into())
        .await
        .unwrap();
    svc.remove_approved_source(ActorId::from(APPROVED_SOURCE))
        .with_actor_id(OWNER.into())
        .await
        .unwrap();

    assert!(!svc
        .is_approved_source(ActorId::from(APPROVED_SOURCE))
        .await
        .unwrap());
}

// ─── RecordCompletion ─────────────────────────────────────────────────────────

#[tokio::test]
async fn record_completion_from_approved_source_updates_ledger() {
    let env = GtestEnv::system_default();
    let program = deploy(&env).await;
    let mut svc = program.reputation_oracle();

    // Grant APPROVED_SOURCE permission
    svc.add_approved_source(ActorId::from(APPROVED_SOURCE))
        .with_actor_id(OWNER.into())
        .await
        .unwrap();

    // Record a successful job worth 500 planck
    let ok = svc
        .record_completion(ActorId::from(WORKER), true, 500)
        .with_actor_id(APPROVED_SOURCE.into())
        .await
        .unwrap();
    assert!(ok);

    let score: ReputationScore = svc.query_score(ActorId::from(WORKER)).await.unwrap();
    assert_eq!(1, score.completed);
    assert_eq!(0, score.disputed);
    assert_eq!(500, score.total_value);
    assert_eq!(10_000, score.success_rate); // 100% — no disputes
}

#[tokio::test]
async fn record_dispute_decrements_success_rate() {
    let env = GtestEnv::system_default();
    let program = deploy(&env).await;
    let mut svc = program.reputation_oracle();

    svc.add_approved_source(ActorId::from(APPROVED_SOURCE))
        .with_actor_id(OWNER.into())
        .await
        .unwrap();

    // 3 completed, 1 disputed → success_rate = 3/4 * 10_000 = 7500 bps
    for _ in 0..3 {
        svc.record_completion(ActorId::from(WORKER), true, 100)
            .with_actor_id(APPROVED_SOURCE.into())
            .await
            .unwrap();
    }
    svc.record_completion(ActorId::from(WORKER), false, 0)
        .with_actor_id(APPROVED_SOURCE.into())
        .await
        .unwrap();

    let score: ReputationScore = svc.query_score(ActorId::from(WORKER)).await.unwrap();
    assert_eq!(3, score.completed);
    assert_eq!(1, score.disputed);
    assert_eq!(7_500, score.success_rate);
}

#[tokio::test]
#[should_panic]
async fn record_completion_from_non_source_panics() {
    let env = GtestEnv::system_default();
    let program = deploy(&env).await;
    let mut svc = program.reputation_oracle();

    // STRANGER is not an approved source
    svc.record_completion(ActorId::from(WORKER), true, 100)
        .with_actor_id(STRANGER.into())
        .await
        .unwrap();
}

#[tokio::test]
#[should_panic]
async fn record_completion_self_loop_panics() {
    let env = GtestEnv::system_default();
    let program = deploy(&env).await;
    let mut svc = program.reputation_oracle();

    svc.add_approved_source(ActorId::from(APPROVED_SOURCE))
        .with_actor_id(OWNER.into())
        .await
        .unwrap();

    // Worker == caller → SelfLoop
    svc.record_completion(ActorId::from(APPROVED_SOURCE), true, 100)
        .with_actor_id(APPROVED_SOURCE.into())
        .await
        .unwrap();
}

// ─── GetScore (paid query) ─────────────────────────────────────────────────────

#[tokio::test]
async fn get_score_exact_fee_succeeds_no_refund() {
    let env = GtestEnv::system_default();
    env.system().mint_to(STRANGER, DEFAULT_USERS_INITIAL_BALANCE);
    let program = deploy(&env).await;
    let mut svc = program.reputation_oracle();

    let score: ReputationScore = svc
        .get_score(ActorId::from(WORKER))
        .with_actor_id(STRANGER.into())
        .with_value(DEFAULT_QUERY_FEE)
        .await
        .unwrap();

    assert_eq!(ActorId::from(WORKER), score.worker);
    assert_eq!(10_000, score.success_rate); // no history = perfect
    // Program collected exactly query_fee (no excess to refund)
    assert_eq!(DEFAULT_QUERY_FEE, svc.collected_fees().await.unwrap());
}

#[tokio::test]
async fn get_score_overpayment_refunds_excess() {
    let env = GtestEnv::system_default();
    let overpay = DEFAULT_QUERY_FEE + 5_000_000_000; // fee + 5 VARA extra
    env.system().mint_to(STRANGER, DEFAULT_USERS_INITIAL_BALANCE);
    let program = deploy(&env).await;
    let mut svc = program.reputation_oracle();

    let score: ReputationScore = svc
        .get_score(ActorId::from(WORKER))
        .with_actor_id(STRANGER.into())
        .with_value(overpay)
        .await
        .unwrap();

    assert_eq!(ActorId::from(WORKER), score.worker);
    // Program only retains query_fee; the excess 5 VARA was refunded via CommandReply::with_value
    assert_eq!(DEFAULT_QUERY_FEE, svc.collected_fees().await.unwrap(), "excess not refunded");
}

#[tokio::test]
#[should_panic]
async fn get_score_insufficient_fee_panics_and_refunds_all() {
    // In Gear, panics auto-refund attached value.
    // gtest surfaces this as a test panic.
    let env = GtestEnv::system_default();
    let underpay = DEFAULT_QUERY_FEE / 2;
    env.system().mint_to(STRANGER, underpay * 2);
    let program = deploy(&env).await;
    let mut svc = program.reputation_oracle();

    svc.get_score(ActorId::from(WORKER))
        .with_actor_id(STRANGER.into())
        .with_value(underpay)
        .await
        .unwrap(); // panics "InsufficientFee"
}

// ─── QueryScore (free read) ────────────────────────────────────────────────────

#[tokio::test]
async fn query_score_unknown_worker_returns_zeros() {
    let env = GtestEnv::system_default();
    let program = deploy(&env).await;
    let svc = program.reputation_oracle();

    let score: ReputationScore = svc.query_score(ActorId::from(WORKER)).await.unwrap();
    assert_eq!(0, score.completed);
    assert_eq!(0, score.disputed);
    assert_eq!(10_000, score.success_rate); // perfect for untested
    assert_eq!(0, score.total_value);
}

#[tokio::test]
async fn query_score_after_record_matches_get_score() {
    let env = GtestEnv::system_default();
    env.system().mint_to(STRANGER, DEFAULT_USERS_INITIAL_BALANCE);
    let program = deploy(&env).await;
    let mut svc = program.reputation_oracle();

    svc.add_approved_source(ActorId::from(APPROVED_SOURCE))
        .with_actor_id(OWNER.into())
        .await
        .unwrap();
    svc.record_completion(ActorId::from(WORKER), true, 1_000)
        .with_actor_id(APPROVED_SOURCE.into())
        .await
        .unwrap();

    let free_score: ReputationScore = svc.query_score(ActorId::from(WORKER)).await.unwrap();
    let paid_score: ReputationScore = svc
        .get_score(ActorId::from(WORKER))
        .with_actor_id(STRANGER.into())
        .with_value(DEFAULT_QUERY_FEE)
        .await
        .unwrap();

    assert_eq!(free_score.completed, paid_score.completed);
    assert_eq!(free_score.success_rate, paid_score.success_rate);
}

// ─── SetQueryFee ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn set_query_fee_by_owner_works() {
    let env = GtestEnv::system_default();
    let program = deploy(&env).await;
    let mut svc = program.reputation_oracle();

    let new_fee = 200_000_000_000u128; // 0.2 VARA
    let ok = svc
        .set_query_fee(new_fee)
        .with_actor_id(OWNER.into())
        .await
        .unwrap();
    assert!(ok);
    assert_eq!(new_fee, svc.query_fee().await.unwrap());
}

#[tokio::test]
#[should_panic]
async fn set_query_fee_by_stranger_panics() {
    let env = GtestEnv::system_default();
    let program = deploy(&env).await;
    let mut svc = program.reputation_oracle();

    svc.set_query_fee(1)
        .with_actor_id(STRANGER.into())
        .await
        .unwrap();
}

// ─── Events ───────────────────────────────────────────────────────────────────

#[tokio::test]
async fn completion_recorded_event_is_emitted() {
    let env = GtestEnv::system_default();
    let program = deploy(&env).await;
    let mut svc = program.reputation_oracle();
    let mut event_stream = svc.listen().await.unwrap();

    svc.add_approved_source(ActorId::from(APPROVED_SOURCE))
        .with_actor_id(OWNER.into())
        .await
        .unwrap();
    svc.record_completion(ActorId::from(WORKER), true, 999)
        .with_actor_id(APPROVED_SOURCE.into())
        .await
        .unwrap();

    // Drain add-source event, then check the completion event
    let _ = event_stream.next().await;
    let (_, ev) = event_stream.next().await.unwrap();
    assert_eq!(
        events::ReputationOracleEvents::CompletionRecorded {
            worker: ActorId::from(WORKER),
            succeeded: true,
            value: 999
        },
        ev
    );
}
