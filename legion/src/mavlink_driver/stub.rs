//! Software-only `MavlinkBackend` impl. Logs every call, keeps a tiny
//! in-memory model of the drone's GPS position + battery so the
//! executor and telemetry pumper see a consistent picture, and
//! resolves every command instantly.
//!
//! This is *not* a SITL driver — it doesn't speak MAVLink and it
//! doesn't connect to a Pixhawk. It's a pure software fake. SITL
//! integration lives in a sibling module to be added after README
//! open question #1.

use std::sync::{Arc, Mutex};

use hivemind_protocol::{Position, Waypoint};
use legion_core::error::MavlinkError;
use legion_core::MavlinkBackend;

#[derive(Debug, Clone)]
struct State {
    position: Position,
    battery_pct: f32,
    armed: bool,
    in_air: bool,
    /// Software model of the AUX5 nozzle servo. `true` = pressed
    /// (spray on), `false` = released (spray off).
    nozzle_open: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            position: Position {
                lat: 0.0,
                lon: 0.0,
                alt_m: 0.0,
            },
            battery_pct: 95.0,
            armed: false,
            in_air: false,
            nozzle_open: false,
        }
    }
}

pub struct StubMavlinkDriver {
    state: Arc<Mutex<State>>,
}

impl StubMavlinkDriver {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(State::default())),
        }
    }

    /// Tell the stub to report a different battery percentage — used
    /// by `legion debug` subcommands to exercise the safety loop.
    pub fn set_battery(&self, pct: f32) {
        self.state.lock().unwrap().battery_pct = pct;
    }

    /// Move the reported position directly — used for tests and
    /// `legion debug fly-to`.
    pub fn teleport(&self, wp: Waypoint) {
        let mut st = self.state.lock().unwrap();
        st.position = Position {
            lat: wp.lat,
            lon: wp.lon,
            alt_m: wp.alt_m,
        };
    }
}

impl Default for StubMavlinkDriver {
    fn default() -> Self {
        Self::new()
    }
}

impl MavlinkBackend for StubMavlinkDriver {
    async fn arm(&self) -> Result<(), MavlinkError> {
        tracing::info!("stub mavlink: arm");
        self.state.lock().unwrap().armed = true;
        Ok(())
    }

    async fn disarm(&self) -> Result<(), MavlinkError> {
        tracing::info!("stub mavlink: disarm");
        let mut st = self.state.lock().unwrap();
        st.armed = false;
        st.in_air = false;
        Ok(())
    }

    async fn takeoff(&self, alt_m: f32) -> Result<(), MavlinkError> {
        tracing::info!("stub mavlink: takeoff to {alt_m} m");
        let mut st = self.state.lock().unwrap();
        if !st.armed {
            return Err(MavlinkError::IllegalState {
                detail: "takeoff while disarmed".into(),
            });
        }
        st.position.alt_m = alt_m;
        st.in_air = true;
        Ok(())
    }

    async fn goto(&self, wp: Waypoint, speed_m_s: f32) -> Result<(), MavlinkError> {
        tracing::info!(
            "stub mavlink: goto lat={} lon={} alt={} at {} m/s",
            wp.lat,
            wp.lon,
            wp.alt_m,
            speed_m_s
        );
        self.teleport(wp);
        Ok(())
    }

    async fn follow_path(
        &self,
        path: &[Waypoint],
        speed_m_s: f32,
    ) -> Result<(), MavlinkError> {
        tracing::info!(
            "stub mavlink: follow_path (len={}) at {} m/s",
            path.len(),
            speed_m_s
        );
        if let Some(last) = path.last() {
            self.teleport(*last);
        }
        Ok(())
    }

    async fn return_to_launch(&self) -> Result<(), MavlinkError> {
        tracing::info!("stub mavlink: rtl");
        let mut st = self.state.lock().unwrap();
        st.position = Position {
            lat: 0.0,
            lon: 0.0,
            alt_m: 0.0,
        };
        st.in_air = false;
        Ok(())
    }

    async fn land(&self) -> Result<(), MavlinkError> {
        tracing::info!("stub mavlink: land");
        let mut st = self.state.lock().unwrap();
        st.position.alt_m = 0.0;
        st.in_air = false;
        Ok(())
    }

    async fn hold(&self) -> Result<(), MavlinkError> {
        tracing::info!("stub mavlink: hold");
        Ok(())
    }

    async fn emergency_pullback(&self) -> Result<(), MavlinkError> {
        tracing::warn!("stub mavlink: emergency_pullback");
        Ok(())
    }

    async fn inject_rtk(&self, rtcm: &[u8]) -> Result<(), MavlinkError> {
        tracing::debug!("stub mavlink: inject_rtk ({} bytes)", rtcm.len());
        Ok(())
    }

    async fn set_nozzle(&self, open: bool) -> Result<(), MavlinkError> {
        tracing::info!("stub mavlink: set_nozzle({open}) [AUX5 servo]");
        self.state.lock().unwrap().nozzle_open = open;
        Ok(())
    }

    fn position(&self) -> Position {
        self.state.lock().unwrap().position
    }

    fn battery_pct(&self) -> f32 {
        self.state.lock().unwrap().battery_pct
    }
}
