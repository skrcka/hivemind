//! Integration tests for the sortie executor.

mod common;

use common::{mini_sortie, FakeClock, MavCall, MockLink, MockMavlink, MockPayload, MockStore};
use legion_core::error::CoreError;
use legion_core::traits::link::ExecutorEvent;
use legion_core::{executor::Executor, LegionState, LegionToOracle};

// Helper: build the three fixed-step proceeds for the minimal sortie.
fn proceed(idx: u32) -> ExecutorEvent {
    ExecutorEvent::Proceed {
        sortie_id: "sortie-1".into(),
        expected_step_index: idx,
    }
}

#[tokio::test]
async fn runs_a_clean_sortie_end_to_end() {
    let sortie = mini_sortie();
    let mut payload = MockPayload::healthy();
    let mavlink = MockMavlink::new();
    let store = MockStore::new();
    let clock = FakeClock::new();
    let mut link = MockLink::with_inbound([
        Some(proceed(0)),
        Some(proceed(1)),
        Some(proceed(2)),
    ]);
    let mut state = LegionState::new("drone-01");

    let result = Executor::run_sortie(
        sortie,
        &mut payload,
        &mavlink,
        &store,
        &clock,
        &mut link,
        &mut state,
    )
    .await;

    assert!(result.is_ok(), "sortie should complete cleanly: {result:?}");

    // Three StepComplete frames followed by SortieComplete.
    let step_completes = link
        .outbound
        .iter()
        .filter(|m| matches!(m, LegionToOracle::StepComplete { .. }))
        .count();
    assert_eq!(step_completes, 3);

    assert!(matches!(
        link.outbound.last(),
        Some(LegionToOracle::SortieComplete { .. })
    ));

    // Store was checkpointed 3 times and marked complete.
    assert_eq!(store.progress.lock().unwrap().len(), 3);
    assert!(store.is_completed("sortie-1"));

    // Mavlink saw arm → takeoff → goto → land → disarm (in that order).
    let log = mavlink.call_log();
    assert!(matches!(log[0], MavCall::Arm));
    assert!(matches!(log[1], MavCall::Takeoff(_)));
    assert!(matches!(log[2], MavCall::Goto(_)));
    assert!(matches!(log[3], MavCall::Land));
    assert!(matches!(log[4], MavCall::Disarm));
}

#[tokio::test]
async fn rejects_out_of_order_proceed_and_waits_for_correct_one() {
    let sortie = mini_sortie();
    let mut payload = MockPayload::healthy();
    let mavlink = MockMavlink::new();
    let store = MockStore::new();
    let clock = FakeClock::new();

    // Oracle sends Proceed for step 2 first (wrong — we're on step 0),
    // then the correct ones. Legion should emit an Error and keep
    // waiting. It then accepts the correct Proceed(0) and continues.
    let mut link = MockLink::with_inbound([
        Some(proceed(2)),
        Some(proceed(0)),
        Some(proceed(1)),
        Some(proceed(2)),
    ]);
    let mut state = LegionState::new("drone-01");

    let result = Executor::run_sortie(
        sortie,
        &mut payload,
        &mavlink,
        &store,
        &clock,
        &mut link,
        &mut state,
    )
    .await;

    assert!(result.is_ok());

    // The first outbound frame should be an Error for the out-of-order
    // proceed.
    let first_error = link
        .outbound
        .iter()
        .find(|m| matches!(m, LegionToOracle::Error { .. }));
    assert!(first_error.is_some());
    match first_error.unwrap() {
        LegionToOracle::Error { code, .. } => assert_eq!(code, "proceed_out_of_order"),
        _ => unreachable!(),
    }
}

#[tokio::test]
async fn abort_sortie_before_first_step_emits_failed_and_rtls() {
    let sortie = mini_sortie();
    let mut payload = MockPayload::healthy();
    let mavlink = MockMavlink::new();
    let store = MockStore::new();
    let clock = FakeClock::new();
    let mut link = MockLink::with_inbound([Some(ExecutorEvent::AbortSortie {
        sortie_id: "sortie-1".into(),
        reason: "operator aborted".into(),
    })]);
    let mut state = LegionState::new("drone-01");

    let result = Executor::run_sortie(
        sortie,
        &mut payload,
        &mavlink,
        &store,
        &clock,
        &mut link,
        &mut state,
    )
    .await;

    assert!(matches!(result, Err(CoreError::AbortedByOracle { .. })));

    // A SortieFailed went out.
    assert!(link
        .outbound
        .iter()
        .any(|m| matches!(m, LegionToOracle::SortieFailed { .. })));

    // RTL was commanded.
    assert!(mavlink
        .call_log()
        .iter()
        .any(|c| matches!(c, MavCall::Rtl)));
}

#[tokio::test]
async fn hold_step_blocks_then_proceed_resumes() {
    let sortie = mini_sortie();
    let mut payload = MockPayload::healthy();
    let mavlink = MockMavlink::new();
    let store = MockStore::new();
    let clock = FakeClock::new();

    // Step 0: HoldStep then Proceed.
    // Steps 1 and 2: direct Proceed.
    let mut link = MockLink::with_inbound([
        Some(ExecutorEvent::HoldStep {
            sortie_id: "sortie-1".into(),
            reason: "operator paused".into(),
        }),
        Some(proceed(0)),
        Some(proceed(1)),
        Some(proceed(2)),
    ]);
    let mut state = LegionState::new("drone-01");

    let result = Executor::run_sortie(
        sortie,
        &mut payload,
        &mavlink,
        &store,
        &clock,
        &mut link,
        &mut state,
    )
    .await;

    assert!(result.is_ok());

    // A Held frame was emitted.
    assert!(link
        .outbound
        .iter()
        .any(|m| matches!(m, LegionToOracle::Held { .. })));

    // Mavlink hold was commanded while waiting (before the final land).
    assert!(mavlink
        .call_log()
        .iter()
        .any(|c| matches!(c, MavCall::Hold)));
}

#[tokio::test]
async fn radio_loss_hold_then_rtl_fires_when_oracle_silent_on_takeoff() {
    // Step 0 of mini_sortie is Takeoff with HoldThenRtl policy.
    let sortie = mini_sortie();
    let mut payload = MockPayload::healthy();
    let mavlink = MockMavlink::new();
    let store = MockStore::new();
    let clock = FakeClock::new();

    // First recv: timeout (None). Second recv (during the hold wait
    // inside radio_loss): still timeout. Then the link reports still
    // connected so the policy will RTL. Except — MockLink reports
    // connected = true even during a scripted timeout. The radio_loss
    // policy checks `link.is_connected()` after the hold delay.
    // For this test we force the link to be disconnected for the
    // radio-loss window.
    let mut link = MockLink::with_inbound([None]);
    link.connected = false;

    let mut state = LegionState::new("drone-01");

    let result = Executor::run_sortie(
        sortie,
        &mut payload,
        &mavlink,
        &store,
        &clock,
        &mut link,
        &mut state,
    )
    .await;

    // HoldThenRtl with disconnected link → executor terminates
    // successfully with a radio-loss-applied outcome.
    assert!(result.is_ok());

    // Mavlink should see Hold and then Rtl.
    let log = mavlink.call_log();
    assert!(log.iter().any(|c| matches!(c, MavCall::Hold)));
    assert!(log.iter().any(|c| matches!(c, MavCall::Rtl)));
}

#[tokio::test]
async fn return_to_base_overrides_current_step() {
    let sortie = mini_sortie();
    let mut payload = MockPayload::healthy();
    let mavlink = MockMavlink::new();
    let store = MockStore::new();
    let clock = FakeClock::new();
    let mut link = MockLink::with_inbound([Some(ExecutorEvent::ReturnToBase {
        reason: "weather closed".into(),
    })]);
    let mut state = LegionState::new("drone-01");

    let result = Executor::run_sortie(
        sortie,
        &mut payload,
        &mavlink,
        &store,
        &clock,
        &mut link,
        &mut state,
    )
    .await;

    assert!(matches!(result, Err(CoreError::RtlByOracle { .. })));
    assert!(mavlink
        .call_log()
        .iter()
        .any(|c| matches!(c, MavCall::Rtl)));
}

#[tokio::test]
async fn cancel_sortie_returns_cleanly_without_rtl() {
    let sortie = mini_sortie();
    let mut payload = MockPayload::healthy();
    let mavlink = MockMavlink::new();
    let store = MockStore::new();
    let clock = FakeClock::new();
    let mut link = MockLink::with_inbound([Some(ExecutorEvent::CancelSortie {
        sortie_id: "sortie-1".into(),
    })]);
    let mut state = LegionState::new("drone-01");

    let result = Executor::run_sortie(
        sortie,
        &mut payload,
        &mavlink,
        &store,
        &clock,
        &mut link,
        &mut state,
    )
    .await;

    assert!(result.is_ok());
    // No RTL commanded.
    assert!(!mavlink
        .call_log()
        .iter()
        .any(|c| matches!(c, MavCall::Rtl)));
    assert!(state.current_sortie.is_none());
}
