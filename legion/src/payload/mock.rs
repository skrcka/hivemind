//! Software-only payload implementation. Used on dev machines, in
//! SITL, and for unit tests. Tracks software sensor state only — no
//! hardware I/O.
//!
//! v1 payload is only two sensors: the forward ToF (healthy default
//! 150 cm) and paint level (reports `NotInstalled` so the safety
//! check's paint-empty branch is skipped — v1 has no HX711 load cell,
//! see `hw/nozzle/README.md`). The nozzle is a Pixhawk AUX5 actuator
//! and lives on `MavlinkBackend::set_nozzle`, not here.

use std::sync::{Arc, Mutex};

use legion_core::error::PayloadError;
use legion_core::{PaintLevel, Payload, Tof};

/// ToF with a settable current reading. The safety-loop integration
/// tests flip this under the threshold to drive a trip.
#[derive(Debug, Clone)]
pub struct MockTof {
    current: Arc<Mutex<f32>>,
}

impl MockTof {
    pub fn new_at(cm: f32) -> Self {
        Self {
            current: Arc::new(Mutex::new(cm)),
        }
    }
    pub fn set(&self, cm: f32) {
        *self.current.lock().unwrap() = cm;
    }
}

impl Default for MockTof {
    fn default() -> Self {
        Self::new_at(150.0)
    }
}

impl Tof for MockTof {
    async fn read_cm(&mut self) -> Result<f32, PayloadError> {
        Ok(*self.current.lock().unwrap())
    }
}

/// v1 paint-level sensor: always `NotInstalled`. v1 has no HX711
/// load cell; the operator counts seconds and swaps cans manually.
/// The trait impl is still required because the safety check reads
/// through it, but the impl returns `NotInstalled` so the check is
/// skipped.
#[derive(Debug, Default, Clone)]
pub struct NotInstalledPaintLevel;

impl PaintLevel for NotInstalledPaintLevel {
    async fn read_ml(&mut self) -> Result<f32, PayloadError> {
        Err(PayloadError::NotInstalled)
    }
}

/// Bundle of the v1 payload sub-devices. Nozzle is **not** here —
/// it's a Pixhawk AUX5 actuator, driven via `MavlinkBackend`.
#[derive(Default)]
pub struct MockPayload {
    pub tof: MockTof,
    pub paint_level: NotInstalledPaintLevel,
}

impl MockPayload {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Payload for MockPayload {
    type Tof = MockTof;
    type PaintLevel = NotInstalledPaintLevel;

    fn tof(&mut self) -> &mut Self::Tof {
        &mut self.tof
    }
    fn paint_level(&mut self) -> &mut Self::PaintLevel {
        &mut self.paint_level
    }
}
