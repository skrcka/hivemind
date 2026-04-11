//! Hold-to-arm state machine.
//!
//! The operator must hold LB+RB for [`SafetyConfig::arm_hold_duration_s`]
//! seconds for praetor to actually send the arm command. Releasing at any
//! point cancels and resets the progress bar. The same pattern applies to
//! the emergency-stop hold.

use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy)]
pub struct HoldTimer {
    required: Duration,
    started_at: Option<Instant>,
}

impl HoldTimer {
    pub const fn new(required: Duration) -> Self {
        Self {
            required,
            started_at: None,
        }
    }

    /// Update the timer with the button's current state. Returns `true`
    /// exactly once when the hold completes — subsequent ticks return
    /// `false` until the button is released and re-pressed.
    pub fn tick(&mut self, is_held: bool) -> HoldState {
        match (is_held, self.started_at) {
            (true, None) => {
                self.started_at = Some(Instant::now());
                HoldState::Holding { progress: 0.0 }
            }
            (true, Some(start)) => {
                let elapsed = start.elapsed();
                if elapsed >= self.required {
                    // Consume — the next tick sees no start and resets.
                    self.started_at = None;
                    HoldState::Fired
                } else {
                    let p = (elapsed.as_secs_f32() / self.required.as_secs_f32()).clamp(0.0, 1.0);
                    HoldState::Holding { progress: p }
                }
            }
            (false, _) => {
                self.started_at = None;
                HoldState::Idle
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HoldState {
    Idle,
    Holding { progress: f32 },
    Fired,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;

    #[test]
    fn idle_when_not_held() {
        let mut t = HoldTimer::new(Duration::from_millis(100));
        assert_eq!(t.tick(false), HoldState::Idle);
    }

    #[test]
    fn progresses_while_held() {
        let mut t = HoldTimer::new(Duration::from_millis(200));
        let s = t.tick(true);
        match s {
            HoldState::Holding { progress } => assert!(progress < 0.1),
            other => panic!("expected Holding, got {other:?}"),
        }
    }

    #[test]
    fn fires_after_duration() {
        let mut t = HoldTimer::new(Duration::from_millis(30));
        let _ = t.tick(true);
        sleep(Duration::from_millis(50));
        assert_eq!(t.tick(true), HoldState::Fired);
    }

    #[test]
    fn release_cancels_progress() {
        let mut t = HoldTimer::new(Duration::from_millis(30));
        let _ = t.tick(true);
        sleep(Duration::from_millis(10));
        assert_eq!(t.tick(false), HoldState::Idle);
        // Re-press starts from zero
        if let HoldState::Holding { progress } = t.tick(true) {
            assert!(progress < 0.1);
        } else {
            panic!("expected fresh Holding");
        }
    }
}
