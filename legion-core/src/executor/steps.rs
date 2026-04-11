//! Per-`StepType` handlers. Each handler owns the MAVLink sequence and
//! pump/nozzle state changes for its step type and returns a
//! [`StepOutcome`] on success (or a `CoreError` on failure).
//!
//! All handlers are `&mut`-free on the `MavlinkBackend` (the trait is
//! `&self`-only) but `&mut` on the `Payload` (because pump/nozzle state
//! is tracked in software).

use core::time::Duration;

use crate::error::CoreError;
use crate::traits::{Clock, MavlinkBackend, Nozzle, Payload, Pump};
use hivemind_protocol::{SortieStep, StepType};

/// Per-step result. The duration is wall-clock time elapsed from the
/// start of the handler to the moment it returned; it's reported in the
/// outbound `StepComplete` frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StepOutcome {
    pub duration: Duration,
}

/// Execute a single step. Dispatches on `step.step_type` into the
/// right handler, applies the spray flag, and returns the duration.
pub async fn run_step<P, M, C>(
    step: &SortieStep,
    payload: &mut P,
    mavlink: &M,
    clock: &C,
) -> Result<StepOutcome, CoreError>
where
    P: Payload,
    M: MavlinkBackend,
    C: Clock,
{
    let start_ms = clock.now_ms();

    // The spray flag on the step is authoritative: the handler opens the
    // nozzle and starts the pump at entry if `step.spray` is true, and
    // closes/stops them on exit. Safety may still override both at any
    // point, but that's out of our hands.
    if step.spray {
        payload.nozzle().open().await?;
        payload.pump().on().await?;
    }

    let result = match step.step_type {
        StepType::Takeoff => do_takeoff(step, mavlink).await,
        StepType::Transit => do_transit(step, mavlink).await,
        StepType::SprayPass => do_spray_pass(step, mavlink).await,
        StepType::RefillApproach => do_refill_approach(step, mavlink).await,
        StepType::RefillWait => do_refill_wait(step, clock).await,
        StepType::ReturnToBase => do_return_to_base(mavlink).await,
        StepType::Land => do_land(mavlink).await,
    };

    // Always drop spray on exit — even on error. The safety loop would
    // catch this eventually, but being explicit here is cheap.
    if step.spray {
        let _ = payload.pump().off().await;
        let _ = payload.nozzle().close().await;
    }

    result?;

    let elapsed = Duration::from_millis(clock.now_ms().saturating_sub(start_ms));
    Ok(StepOutcome { duration: elapsed })
}

async fn do_takeoff<M: MavlinkBackend>(step: &SortieStep, mavlink: &M) -> Result<(), CoreError> {
    mavlink.arm().await?;
    mavlink.takeoff(step.waypoint.alt_m).await?;
    Ok(())
}

async fn do_transit<M: MavlinkBackend>(step: &SortieStep, mavlink: &M) -> Result<(), CoreError> {
    match &step.path {
        Some(path) if !path.is_empty() => {
            mavlink.follow_path(path, step.speed_m_s).await?;
        }
        _ => {
            mavlink.goto(step.waypoint, step.speed_m_s).await?;
        }
    }
    Ok(())
}

async fn do_spray_pass<M: MavlinkBackend>(
    step: &SortieStep,
    mavlink: &M,
) -> Result<(), CoreError> {
    // SprayPass always has a multi-point path; the slicer should never
    // emit a zero-length spray. If it does, degrade to a single goto so
    // the drone doesn't just sit there.
    match &step.path {
        Some(path) if !path.is_empty() => {
            mavlink.follow_path(path, step.speed_m_s).await?;
        }
        _ => {
            mavlink.goto(step.waypoint, step.speed_m_s).await?;
        }
    }
    Ok(())
}

async fn do_refill_approach<M: MavlinkBackend>(
    step: &SortieStep,
    mavlink: &M,
) -> Result<(), CoreError> {
    mavlink.goto(step.waypoint, step.speed_m_s).await?;
    mavlink.hold().await?;
    Ok(())
}

async fn do_refill_wait<C: Clock>(step: &SortieStep, clock: &C) -> Result<(), CoreError> {
    // Refill wait is a time-based hold. The ground crew is topping up
    // the can; legion just sits in loiter. The slicer sets
    // `expected_duration_s` as the ground-crew estimate; we wait for
    // that long, then return. Oracle gates the transition to the next
    // step as usual.
    let dur = Duration::from_secs(u64::from(step.expected_duration_s));
    clock.sleep(dur).await;
    Ok(())
}

async fn do_return_to_base<M: MavlinkBackend>(mavlink: &M) -> Result<(), CoreError> {
    mavlink.return_to_launch().await?;
    Ok(())
}

async fn do_land<M: MavlinkBackend>(mavlink: &M) -> Result<(), CoreError> {
    mavlink.land().await?;
    mavlink.disarm().await?;
    Ok(())
}
