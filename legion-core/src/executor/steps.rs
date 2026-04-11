//! Per-`StepType` handlers. Each handler owns the MAVLink sequence
//! for its step type and returns a [`StepOutcome`] on success (or a
//! `CoreError` on failure).
//!
//! Spray control is a single `MavlinkBackend::set_nozzle` call — the
//! v1 spray mechanism is a servo on Pixhawk AUX5 (see
//! `hw/nozzle/README.md`), so spraying is commanded through the
//! MAVLink backend, not through a Pi-side `Payload` trait. The
//! `Payload` bundle in this module exists only for forward-passing
//! to `run_step`'s signature stability with sensor-using handlers
//! we may add later.

use core::time::Duration;

use crate::error::CoreError;
use crate::traits::{Clock, MavlinkBackend, Payload};
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
    let _ = payload; // reserved for future sensor-using step handlers
    let start_ms = clock.now_ms();

    // The spray flag on the step is authoritative: the handler opens
    // the nozzle at entry if `step.spray` is true, and closes it on
    // exit. The nozzle is a Pixhawk AUX5 actuator, commanded through
    // the MAVLink backend — see `hw/nozzle/README.md`. Safety may
    // still override the nozzle at any point.
    if step.spray {
        mavlink.set_nozzle(true).await?;
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

    // Always drop spray on exit — even on error. The safety loop
    // would catch this eventually, but being explicit here is cheap.
    if step.spray {
        let _ = mavlink.set_nozzle(false).await;
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
