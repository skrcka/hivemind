//! Payload hardware traits: pump, nozzle servo, forward ToF, paint level.
//!
//! These four are the hardware the Pixhawk doesn't see — they're wired to
//! the Pi's GPIO/PWM/I²C/SPI on v1, and the legion binary drives them via
//! `rppal`. On an MCU they'd be driven via `embedded-hal`.
//!
//! The [`Payload`] super-trait is an associated-type bundle that lets the
//! executor name one concrete thing ("the payload") and borrow each
//! sub-device from it. This is the non-dyn shape stable async fn in trait
//! forces on us; it works cleanly for v1 where exactly one backend is
//! compiled in.

use core::future::Future;

use crate::error::PayloadError;

/// The spray pump. Simple on/off. The pump is the **first** thing the
/// safety loop cuts on any anomaly.
pub trait Pump: Send {
    fn on(&mut self) -> impl Future<Output = Result<(), PayloadError>> + Send;
    fn off(&mut self) -> impl Future<Output = Result<(), PayloadError>> + Send;
    /// Last commanded state. Does *not* read back the hardware — legion's
    /// software state of record.
    fn is_on(&self) -> bool;
}

/// The nozzle servo. v1 uses an SG90 pressing an aerosol can valve —
/// "open" = pressing, "close" = released. A PWM-driven device on the Pi.
pub trait Nozzle: Send {
    fn open(&mut self) -> impl Future<Output = Result<(), PayloadError>> + Send;
    fn close(&mut self) -> impl Future<Output = Result<(), PayloadError>> + Send;
    fn is_open(&self) -> bool;
}

/// Forward-facing time-of-flight sensor. v1 uses VL53L1X over I²C. Returns
/// the current distance in centimetres, or a `PayloadError` if the read
/// failed.
pub trait Tof: Send {
    fn read_cm(&mut self) -> impl Future<Output = Result<f32, PayloadError>> + Send;
}

/// Paint-remaining sensor. v1 is a float-switch or analog ADC reading;
/// returns millilitres remaining. The safety loop compares this against
/// `SafetyConfig::paint_empty_ml`.
pub trait PaintLevel: Send {
    fn read_ml(&mut self) -> impl Future<Output = Result<f32, PayloadError>> + Send;
}

/// Bundle of the four payload devices. The executor borrows individual
/// sub-devices from this one handle.
///
/// Implementers define one associated type per sub-device and one
/// `&mut`-returning accessor. The reason for the associated-type style
/// (rather than `Box<dyn Pump>` etc) is stable async fn in trait:
/// `&dyn Pump` isn't object-safe with async methods.
pub trait Payload: Send {
    type Pump: Pump;
    type Nozzle: Nozzle;
    type Tof: Tof;
    type PaintLevel: PaintLevel;

    fn pump(&mut self) -> &mut Self::Pump;
    fn nozzle(&mut self) -> &mut Self::Nozzle;
    fn tof(&mut self) -> &mut Self::Tof;
    fn paint_level(&mut self) -> &mut Self::PaintLevel;
}
