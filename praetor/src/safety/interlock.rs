//! Pump interlock + other "is this command safe to send right now?" checks.
//!
//! Every guarded command goes through one of the predicates here at
//! dispatch time. The predicate returns either `Ok(())` (send it) or
//! `Err(PraetorError::Interlock(...))` (refuse and tell the UI why).

use crate::error::{PraetorError, Result};
use crate::mavlink_link::snapshot::TelemetrySnapshot;
use crate::state::{ArmingKind, ArmingState, LinkStatus};

/// Guard for `DO_SET_ACTUATOR` pump-on commands.
///
/// Refuses the command unless:
///
///   1. The link is `Connected` (not `Stale`, not `Disconnected`).
///   2. The drone is armed.
///   3. The reported relative altitude is ≥ `minimum_altitude_m`.
///
/// Disarming / landing path commands are allowed through at any altitude —
/// this predicate is *only* called for the "turn pump on" path.
pub fn guard_pump_on(
    link: LinkStatus,
    arming: ArmingState,
    snap: &TelemetrySnapshot,
    minimum_altitude_m: f32,
) -> Result<()> {
    if link != LinkStatus::Connected {
        return Err(PraetorError::Interlock(format!(
            "pump refused: link is {link:?}, expected Connected"
        )));
    }
    if arming.kind != ArmingKind::Armed {
        return Err(PraetorError::Interlock(
            "pump refused: drone is not armed".into(),
        ));
    }
    if snap.position.relative_alt_m < minimum_altitude_m {
        return Err(PraetorError::Interlock(format!(
            "pump refused: altitude {:.2} m < minimum {:.2} m",
            snap.position.relative_alt_m, minimum_altitude_m
        )));
    }
    Ok(())
}

/// Guard for any `MANUAL_CONTROL` stick output.
///
/// Refuses to forward stick inputs unless the link is Connected. Mode
/// handoff is *not* enforced here in v1 — if the operator has switched
/// PX4 out of Offboard manually, PX4 itself decides whether to honour
/// `MANUAL_CONTROL`. We surface the mode in the HUD so the operator knows.
pub fn guard_manual_control(link: LinkStatus) -> Result<()> {
    if link != LinkStatus::Connected {
        return Err(PraetorError::Interlock(format!(
            "stick input refused: link is {link:?}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mavlink_link::snapshot::Position;

    fn snap_at_altitude(alt: f32) -> TelemetrySnapshot {
        let mut s = TelemetrySnapshot::default();
        s.position = Position {
            lat_deg: 0.0,
            lon_deg: 0.0,
            alt_msl_m: 0.0,
            relative_alt_m: alt,
            vx_m_s: 0.0,
            vy_m_s: 0.0,
            vz_m_s: 0.0,
        };
        s
    }

    #[test]
    fn pump_on_refused_if_disarmed() {
        let r = guard_pump_on(
            LinkStatus::Connected,
            ArmingState::disarmed(),
            &snap_at_altitude(2.0),
            0.5,
        );
        assert!(r.is_err());
    }

    #[test]
    fn pump_on_refused_on_ground() {
        let r = guard_pump_on(
            LinkStatus::Connected,
            ArmingState::armed(),
            &snap_at_altitude(0.1),
            0.5,
        );
        assert!(r.is_err());
    }

    #[test]
    fn pump_on_refused_when_stale() {
        let r = guard_pump_on(
            LinkStatus::Stale,
            ArmingState::armed(),
            &snap_at_altitude(2.0),
            0.5,
        );
        assert!(r.is_err());
    }

    #[test]
    fn pump_on_ok_when_all_conditions_met() {
        let r = guard_pump_on(
            LinkStatus::Connected,
            ArmingState::armed(),
            &snap_at_altitude(2.0),
            0.5,
        );
        assert!(r.is_ok());
    }
}
