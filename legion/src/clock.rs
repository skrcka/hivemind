//! Tokio-backed `legion_core::Clock` impl. A monotonic ms source via
//! `tokio::time::Instant::elapsed_since_origin`, and an async sleep via
//! `tokio::time::sleep`.

use std::time::Duration;

use legion_core::Clock;
use tokio::time::Instant;

/// Wraps a `tokio::time::Instant` captured at construction time as the
/// "epoch". All `now_ms` reads are relative to that origin.
#[derive(Debug, Clone)]
pub struct TokioClock {
    origin: Instant,
}

impl TokioClock {
    pub fn new() -> Self {
        Self {
            origin: Instant::now(),
        }
    }
}

impl Default for TokioClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for TokioClock {
    fn now_ms(&self) -> u64 {
        u64::try_from(self.origin.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    async fn sleep(&self, dur: Duration) {
        tokio::time::sleep(dur).await;
    }
}
