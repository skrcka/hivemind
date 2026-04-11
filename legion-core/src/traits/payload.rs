//! Payload hardware traits: forward ToF and paint-level sensors.
//!
//! These are the Pi-side sensors legion reads during the safety loop
//! and on entry to the safety check. On v1 the only installed sensor
//! is the forward ToF (VL53L1X over I²C on the Pi 5). Paint level is
//! kept as a trait for v2 readiness (HX711 load cell under the spray
//! can), but v1 impls return `PayloadError::NotInstalled` and the
//! safety check skips the paint-empty threshold.
//!
//! **The nozzle is not here.** v1 drives the SG90 servo through
//! Pixhawk AUX5, and legion commands it via the [`MavlinkBackend`]
//! trait (`set_nozzle`). See `hw/nozzle/README.md` for the wiring and
//! `legion/README.md` for the architectural rationale.
//!
//! [`MavlinkBackend`]: crate::traits::MavlinkBackend

use core::future::Future;

use crate::error::PayloadError;

/// Forward-facing time-of-flight sensor. v1 uses VL53L1X over I²C.
/// Returns the current distance in centimetres, or a `PayloadError`
/// if the read failed or the sensor isn't installed.
pub trait Tof: Send {
    fn read_cm(&mut self) -> impl Future<Output = Result<f32, PayloadError>> + Send;
}

/// Paint-remaining sensor (v2-only). v1 has no paint-level hardware —
/// the operator counts seconds of spray and swaps the can when in
/// doubt, and the v1 impl returns `PayloadError::NotInstalled` so the
/// safety loop's paint-empty check is a no-op. The trait is kept
/// in-place so the v2 peristaltic-pump payload can plug in its HX711
/// load-cell reader without touching `legion-core`.
pub trait PaintLevel: Send {
    fn read_ml(&mut self) -> impl Future<Output = Result<f32, PayloadError>> + Send;
}

/// Bundle of the Pi-side sensor sub-devices. The executor borrows
/// individual sub-devices from this one handle.
///
/// Nozzle / pump control is **not** part of this trait — the nozzle
/// is a Pixhawk AUX5 actuator, driven through `MavlinkBackend`. The
/// bundle exists so the safety loop can read every Pi-side sensor
/// through one handle.
pub trait Payload: Send {
    type Tof: Tof;
    type PaintLevel: PaintLevel;

    fn tof(&mut self) -> &mut Self::Tof;
    fn paint_level(&mut self) -> &mut Self::PaintLevel;
}
