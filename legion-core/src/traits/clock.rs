//! Clock abstraction.
//!
//! `legion-core` never imports `std::time::Instant` or `tokio::time` —
//! neither is available on the MCU target. Instead the executor and
//! safety loop express durations in "milliseconds since boot" through
//! this trait. The Pi binary backs it with `tokio::time::Instant`; an
//! MCU binary would back it with `embassy_time::Instant`.

use core::time::Duration;

/// A monotonic millisecond clock and a runtime-agnostic `sleep`.
///
/// Only `now_ms` and `sleep` need to be async-free or async-capable; the
/// futures returned are polled by whatever runtime hosts the core.
pub trait Clock: Send + Sync {
    /// Monotonic milliseconds since some implementation-defined epoch
    /// (typically boot). Never goes backwards, never wraps in practice.
    fn now_ms(&self) -> u64;

    /// Async sleep for at least `dur`. The binary wires this to
    /// `tokio::time::sleep` or `embassy_time::Timer::after`.
    fn sleep(&self, dur: Duration) -> impl core::future::Future<Output = ()> + Send;

    /// Milliseconds elapsed since `since_ms`. Saturates at 0 if the
    /// clock was reset (shouldn't happen, but don't underflow).
    fn elapsed_ms(&self, since_ms: u64) -> u64 {
        self.now_ms().saturating_sub(since_ms)
    }
}
