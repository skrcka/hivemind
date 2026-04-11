//! Watchdog checks — whether the controller or the link has gone quiet.

use std::time::{Duration, Instant};

use crate::gamepad::intent::ControlIntent;
use crate::mavlink_link::snapshot::TelemetrySnapshot;

/// `true` if the controller has been silent longer than `threshold`.
pub fn controller_silent(intent: &ControlIntent, threshold: Duration) -> bool {
    match intent.last_event_at {
        None => true, // never saw an event
        Some(t) => t.elapsed() > threshold,
    }
}

/// `true` if the last HEARTBEAT was more than `threshold` ago.
/// `now_ms` is the current wall-clock ms from the caller (we pass it in
/// explicitly rather than calling SystemTime inside so tests are deterministic).
pub fn link_silent(snap: &TelemetrySnapshot, threshold: Duration, now_ms: u64) -> bool {
    if snap.updated_at_ms == 0 {
        return true;
    }
    let elapsed = now_ms.saturating_sub(snap.updated_at_ms);
    elapsed > threshold.as_millis() as u64
}

/// Return the elapsed time since the most recent telemetry update, computed
/// against `now`. Used by the HUD's staleness indicator.
pub fn telemetry_age(snap: &TelemetrySnapshot, now: Instant) -> Option<Duration> {
    if snap.updated_at_ms == 0 {
        None
    } else {
        Some(
            now.duration_since(Instant::now())
                .saturating_add(Duration::ZERO),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controller_silent_when_no_events() {
        let c = ControlIntent::neutral();
        assert!(controller_silent(&c, Duration::from_millis(100)));
    }

    #[test]
    fn link_silent_with_fresh_telemetry() {
        let mut s = TelemetrySnapshot::default();
        s.updated_at_ms = 1000;
        let threshold = Duration::from_secs(2);
        assert!(!link_silent(&s, threshold, 1500)); // 500 ms old
        assert!(link_silent(&s, threshold, 5000)); // 4000 ms old
    }
}
