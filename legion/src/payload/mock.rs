//! Software-only payload implementation. Used on dev machines, in
//! SITL, and for unit tests. Tracks software state only — no hardware
//! I/O.
//!
//! Sensor readings are "healthy by default": ToF reports a safe 150 cm
//! distance, paint level reports a full 500 ml tank. The `legion
//! debug` subcommands can nudge these values for scripted tests.

use std::sync::{Arc, Mutex};

use legion_core::error::PayloadError;
use legion_core::{Nozzle, PaintLevel, Payload, Pump, Tof};

#[derive(Debug, Default)]
pub struct MockPump {
    pub on: bool,
}

impl Pump for MockPump {
    async fn on(&mut self) -> Result<(), PayloadError> {
        tracing::debug!("mock pump: on");
        self.on = true;
        Ok(())
    }
    async fn off(&mut self) -> Result<(), PayloadError> {
        tracing::debug!("mock pump: off");
        self.on = false;
        Ok(())
    }
    fn is_on(&self) -> bool {
        self.on
    }
}

#[derive(Debug, Default)]
pub struct MockNozzle {
    pub open: bool,
}

impl Nozzle for MockNozzle {
    async fn open(&mut self) -> Result<(), PayloadError> {
        tracing::debug!("mock nozzle: open");
        self.open = true;
        Ok(())
    }
    async fn close(&mut self) -> Result<(), PayloadError> {
        tracing::debug!("mock nozzle: close");
        self.open = false;
        Ok(())
    }
    fn is_open(&self) -> bool {
        self.open
    }
}

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

#[derive(Debug, Clone)]
pub struct MockPaintLevel {
    current: Arc<Mutex<f32>>,
}

impl MockPaintLevel {
    pub fn new_at(ml: f32) -> Self {
        Self {
            current: Arc::new(Mutex::new(ml)),
        }
    }
    pub fn set(&self, ml: f32) {
        *self.current.lock().unwrap() = ml;
    }
}

impl Default for MockPaintLevel {
    fn default() -> Self {
        Self::new_at(500.0)
    }
}

impl PaintLevel for MockPaintLevel {
    async fn read_ml(&mut self) -> Result<f32, PayloadError> {
        Ok(*self.current.lock().unwrap())
    }
}

/// Bundle of the four mock sub-devices.
pub struct MockPayload {
    pub pump: MockPump,
    pub nozzle: MockNozzle,
    pub tof: MockTof,
    pub paint_level: MockPaintLevel,
}

impl MockPayload {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Default for MockPayload {
    fn default() -> Self {
        Self {
            pump: MockPump::default(),
            nozzle: MockNozzle::default(),
            tof: MockTof::default(),
            paint_level: MockPaintLevel::default(),
        }
    }
}

impl Payload for MockPayload {
    type Pump = MockPump;
    type Nozzle = MockNozzle;
    type Tof = MockTof;
    type PaintLevel = MockPaintLevel;

    fn pump(&mut self) -> &mut Self::Pump {
        &mut self.pump
    }
    fn nozzle(&mut self) -> &mut Self::Nozzle {
        &mut self.nozzle
    }
    fn tof(&mut self) -> &mut Self::Tof {
        &mut self.tof
    }
    fn paint_level(&mut self) -> &mut Self::PaintLevel {
        &mut self.paint_level
    }
}
