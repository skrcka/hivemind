//! Shared test helpers. Mock impls for every legion-core trait, plus a
//! `FakeClock` that ticks via a `std::sync::atomic::AtomicU64` so tests
//! can advance time deterministically.

#![allow(dead_code)]

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use legion_core::error::{LinkError, MavlinkError, PayloadError, StoreError};
use legion_core::traits::link::ExecutorEvent;
use legion_core::traits::store::SortieProgress;
use legion_core::{
    Clock, LegionToOracle, Link, MavlinkBackend, PaintLevel, Payload, Sortie, SortieStore, Tof,
};
use legion_core::{Position, Waypoint};

// ─── Clock ────────────────────────────────────────────────────────

/// A deterministic clock for tests. `now_ms` is read from an atomic;
/// `sleep` advances it immediately (no real waiting). Tests explicitly
/// advance time with `tick_ms`.
#[derive(Clone, Default)]
pub struct FakeClock {
    now: Arc<std::sync::atomic::AtomicU64>,
}

impl FakeClock {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn tick_ms(&self, ms: u64) {
        self.now
            .fetch_add(ms, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn set_ms(&self, ms: u64) {
        self.now.store(ms, std::sync::atomic::Ordering::SeqCst);
    }
}

impl Clock for FakeClock {
    fn now_ms(&self) -> u64 {
        self.now.load(std::sync::atomic::Ordering::SeqCst)
    }

    async fn sleep(&self, dur: core::time::Duration) {
        // Tests are deterministic: sleeping just advances the clock.
        let ms = u64::try_from(dur.as_millis()).unwrap_or(u64::MAX);
        self.tick_ms(ms);
    }
}

// ─── Payload ─────────────────────────────────────────────────────

pub struct MockTof {
    /// Scripted readings. Pops from the front; if empty, returns the
    /// last value repeatedly.
    pub readings: VecDeque<f32>,
    last: f32,
}

impl MockTof {
    pub fn new(readings: impl IntoIterator<Item = f32>) -> Self {
        let readings: VecDeque<f32> = readings.into_iter().collect();
        let last = readings.front().copied().unwrap_or(500.0);
        Self { readings, last }
    }
}

impl Tof for MockTof {
    async fn read_cm(&mut self) -> Result<f32, PayloadError> {
        if let Some(v) = self.readings.pop_front() {
            self.last = v;
        }
        Ok(self.last)
    }
}

pub struct MockPaintLevel {
    pub readings: VecDeque<f32>,
    last: f32,
}

impl MockPaintLevel {
    pub fn new(readings: impl IntoIterator<Item = f32>) -> Self {
        let readings: VecDeque<f32> = readings.into_iter().collect();
        let last = readings.front().copied().unwrap_or(1000.0);
        Self { readings, last }
    }
}

impl PaintLevel for MockPaintLevel {
    async fn read_ml(&mut self) -> Result<f32, PayloadError> {
        if let Some(v) = self.readings.pop_front() {
            self.last = v;
        }
        Ok(self.last)
    }
}

pub struct MockPayload {
    pub tof: MockTof,
    pub paint_level: MockPaintLevel,
}

impl MockPayload {
    pub fn healthy() -> Self {
        Self {
            tof: MockTof::new([500.0]),
            paint_level: MockPaintLevel::new([1000.0]),
        }
    }
}

impl Payload for MockPayload {
    type Tof = MockTof;
    type PaintLevel = MockPaintLevel;

    fn tof(&mut self) -> &mut Self::Tof {
        &mut self.tof
    }
    fn paint_level(&mut self) -> &mut Self::PaintLevel {
        &mut self.paint_level
    }
}

// ─── MavlinkBackend ──────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum MavCall {
    Arm,
    Disarm,
    Takeoff(f32),
    Goto(Waypoint),
    FollowPath(Vec<Waypoint>),
    Rtl,
    Land,
    Hold,
    EmergencyPullback,
    InjectRtk(Vec<u8>),
    SetNozzle(bool),
}

pub struct MockMavlink {
    pub calls: Arc<Mutex<Vec<MavCall>>>,
    pub battery_pct: Arc<Mutex<f32>>,
    pub position: Arc<Mutex<Position>>,
    pub nozzle_open: Arc<Mutex<bool>>,
}

impl MockMavlink {
    pub fn new() -> Self {
        Self {
            calls: Arc::new(Mutex::new(Vec::new())),
            battery_pct: Arc::new(Mutex::new(80.0)),
            position: Arc::new(Mutex::new(Position {
                lat: 0.0,
                lon: 0.0,
                alt_m: 0.0,
            })),
            nozzle_open: Arc::new(Mutex::new(false)),
        }
    }

    pub fn record(&self, call: MavCall) {
        self.calls.lock().unwrap().push(call);
    }

    pub fn call_log(&self) -> Vec<MavCall> {
        self.calls.lock().unwrap().clone()
    }

    pub fn set_battery(&self, pct: f32) {
        *self.battery_pct.lock().unwrap() = pct;
    }

    pub fn is_nozzle_open(&self) -> bool {
        *self.nozzle_open.lock().unwrap()
    }
}

impl MavlinkBackend for MockMavlink {
    async fn arm(&self) -> Result<(), MavlinkError> {
        self.record(MavCall::Arm);
        Ok(())
    }
    async fn disarm(&self) -> Result<(), MavlinkError> {
        self.record(MavCall::Disarm);
        Ok(())
    }
    async fn takeoff(&self, alt_m: f32) -> Result<(), MavlinkError> {
        self.record(MavCall::Takeoff(alt_m));
        Ok(())
    }
    async fn goto(&self, wp: Waypoint, _speed: f32) -> Result<(), MavlinkError> {
        self.record(MavCall::Goto(wp));
        Ok(())
    }
    async fn follow_path(&self, path: &[Waypoint], _speed: f32) -> Result<(), MavlinkError> {
        self.record(MavCall::FollowPath(path.to_vec()));
        Ok(())
    }
    async fn return_to_launch(&self) -> Result<(), MavlinkError> {
        self.record(MavCall::Rtl);
        Ok(())
    }
    async fn land(&self) -> Result<(), MavlinkError> {
        self.record(MavCall::Land);
        Ok(())
    }
    async fn hold(&self) -> Result<(), MavlinkError> {
        self.record(MavCall::Hold);
        Ok(())
    }
    async fn emergency_pullback(&self) -> Result<(), MavlinkError> {
        self.record(MavCall::EmergencyPullback);
        Ok(())
    }
    async fn inject_rtk(&self, rtcm: &[u8]) -> Result<(), MavlinkError> {
        self.record(MavCall::InjectRtk(rtcm.to_vec()));
        Ok(())
    }
    async fn set_nozzle(&self, open: bool) -> Result<(), MavlinkError> {
        self.record(MavCall::SetNozzle(open));
        *self.nozzle_open.lock().unwrap() = open;
        Ok(())
    }
    fn position(&self) -> Position {
        *self.position.lock().unwrap()
    }
    fn battery_pct(&self) -> f32 {
        *self.battery_pct.lock().unwrap()
    }
}

// ─── SortieStore ─────────────────────────────────────────────────

#[derive(Default)]
pub struct MockStore {
    pub sorties: Arc<Mutex<Vec<Sortie>>>,
    pub progress: Arc<Mutex<Vec<SortieProgress>>>,
    pub completed: Arc<Mutex<Vec<String>>>,
}

impl MockStore {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn progress_for(&self, sortie_id: &str) -> Option<SortieProgress> {
        self.progress
            .lock()
            .unwrap()
            .iter()
            .rfind(|p| p.sortie_id == sortie_id)
            .cloned()
    }
    pub fn is_completed(&self, sortie_id: &str) -> bool {
        self.completed
            .lock()
            .unwrap()
            .iter()
            .any(|id| id == sortie_id)
    }
}

impl SortieStore for MockStore {
    async fn put(&self, sortie: &Sortie) -> Result<(), StoreError> {
        self.sorties.lock().unwrap().push(sortie.clone());
        Ok(())
    }
    async fn get(&self, sortie_id: &str) -> Result<Sortie, StoreError> {
        self.sorties
            .lock()
            .unwrap()
            .iter()
            .find(|s| s.sortie_id == sortie_id)
            .cloned()
            .ok_or(StoreError::NotFound {
                sortie_id: sortie_id.to_string(),
            })
    }
    async fn checkpoint(&self, progress: &SortieProgress) -> Result<(), StoreError> {
        self.progress.lock().unwrap().push(progress.clone());
        Ok(())
    }
    async fn load_progress(
        &self,
        sortie_id: &str,
    ) -> Result<Option<SortieProgress>, StoreError> {
        Ok(self.progress_for(sortie_id))
    }
    async fn list(&self) -> Result<Vec<String>, StoreError> {
        Ok(self
            .sorties
            .lock()
            .unwrap()
            .iter()
            .map(|s| s.sortie_id.clone())
            .collect())
    }
    async fn mark_complete(&self, sortie_id: &str) -> Result<(), StoreError> {
        self.completed.lock().unwrap().push(sortie_id.to_string());
        Ok(())
    }
}

// ─── Link ────────────────────────────────────────────────────────

pub struct MockLink {
    /// Scripted inbound events the executor will pop one per `recv`.
    /// `None` means "time out on this call".
    pub inbound: VecDeque<Option<ExecutorEvent>>,
    pub outbound: Vec<LegionToOracle>,
    pub connected: bool,
}

impl MockLink {
    pub fn new() -> Self {
        Self {
            inbound: VecDeque::new(),
            outbound: Vec::new(),
            connected: true,
        }
    }

    pub fn with_inbound(events: impl IntoIterator<Item = Option<ExecutorEvent>>) -> Self {
        Self {
            inbound: events.into_iter().collect(),
            outbound: Vec::new(),
            connected: true,
        }
    }

    pub fn disconnected() -> Self {
        Self {
            inbound: VecDeque::new(),
            outbound: Vec::new(),
            connected: false,
        }
    }

    pub fn push(&mut self, event: ExecutorEvent) {
        self.inbound.push_back(Some(event));
    }

    pub fn push_timeout(&mut self) {
        self.inbound.push_back(None);
    }
}

impl Link for MockLink {
    async fn send(&mut self, msg: LegionToOracle) -> Result<(), LinkError> {
        self.outbound.push(msg);
        Ok(())
    }

    async fn recv_executor_event(
        &mut self,
        _timeout: core::time::Duration,
    ) -> Result<Option<ExecutorEvent>, LinkError> {
        // Pop the next scripted entry. If the script is empty, report
        // timeout forever (tests that run past their script hit this).
        Ok(self.inbound.pop_front().unwrap_or(None))
    }

    fn is_connected(&self) -> bool {
        self.connected
    }
}

// ─── Sortie fixtures ─────────────────────────────────────────────

pub fn mini_sortie() -> Sortie {
    use legion_core::{
        RadioLossBehaviour, RadioLossPolicy, SortieStep, StepType, Waypoint,
    };

    Sortie {
        sortie_id: "sortie-1".into(),
        plan_id: "plan-1".into(),
        drone_id: "drone-01".into(),
        paint_volume_ml: 200.0,
        expected_duration_s: 30,
        steps: vec![
            SortieStep {
                index: 0,
                step_type: StepType::Takeoff,
                waypoint: Waypoint {
                    lat: 50.0,
                    lon: 14.0,
                    alt_m: 5.0,
                    yaw_deg: None,
                },
                path: None,
                speed_m_s: 1.0,
                spray: false,
                radio_loss: RadioLossPolicy {
                    behaviour: RadioLossBehaviour::HoldThenRtl,
                    silent_timeout_s: 5.0,
                    hold_then_rtl_after_s: Some(10.0),
                },
                expected_duration_s: 5,
            },
            SortieStep {
                index: 1,
                step_type: StepType::Transit,
                waypoint: Waypoint {
                    lat: 50.001,
                    lon: 14.001,
                    alt_m: 5.0,
                    yaw_deg: None,
                },
                path: None,
                speed_m_s: 3.0,
                spray: false,
                radio_loss: RadioLossPolicy {
                    behaviour: RadioLossBehaviour::Continue,
                    silent_timeout_s: 30.0,
                    hold_then_rtl_after_s: None,
                },
                expected_duration_s: 10,
            },
            SortieStep {
                index: 2,
                step_type: StepType::Land,
                waypoint: Waypoint {
                    lat: 50.0,
                    lon: 14.0,
                    alt_m: 0.0,
                    yaw_deg: None,
                },
                path: None,
                speed_m_s: 0.5,
                spray: false,
                radio_loss: RadioLossPolicy {
                    behaviour: RadioLossBehaviour::Continue,
                    silent_timeout_s: 30.0,
                    hold_then_rtl_after_s: None,
                },
                expected_duration_s: 10,
            },
        ],
    }
}
