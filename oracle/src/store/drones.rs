//! Drone fleet roster persistence.
//!
//! SQLite stores REAL as f64 but the wire types use f32 throughout, so
//! every f64→f32 cast at the load boundary is intentional.

#![allow(clippy::cast_possible_truncation)]

use hivemind_protocol::Telemetry;
use sqlx::Row;
use time::OffsetDateTime;

use crate::domain::fleet::{Drone, DroneState, FleetSnapshot};

use super::Store;

impl Store {
    /// Upsert a drone — called when a Hello arrives or when telemetry comes
    /// in for a known id.
    pub async fn upsert_drone(
        &self,
        id: &str,
        legion_version: Option<&str>,
        capabilities: &[String],
    ) -> Result<(), sqlx::Error> {
        let now = rfc3339_now();
        let caps_json = serde_json::to_string(capabilities).unwrap_or_else(|_| "[]".into());
        sqlx::query(
            "INSERT INTO drones (id, first_seen_at, last_seen_at, legion_version, capabilities) \
             VALUES (?, ?, ?, ?, ?) \
             ON CONFLICT(id) DO UPDATE SET \
                last_seen_at = excluded.last_seen_at, \
                legion_version = COALESCE(excluded.legion_version, legion_version), \
                capabilities = COALESCE(excluded.capabilities, capabilities), \
                is_stale = 0",
        )
        .bind(id)
        .bind(&now)
        .bind(&now)
        .bind(legion_version)
        .bind(&caps_json)
        .execute(self.pool())
        .await?;
        Ok(())
    }

    /// Update a drone's last-known state from a Telemetry frame.
    pub async fn record_telemetry(&self, id: &str, t: &Telemetry) -> Result<(), sqlx::Error> {
        let now = rfc3339_now();
        sqlx::query(
            "UPDATE drones SET \
                last_seen_at = ?, \
                last_known_battery_pct = ?, \
                last_known_paint_ml = ?, \
                last_known_position_lat = ?, \
                last_known_position_lon = ?, \
                last_known_position_alt_m = ?, \
                last_known_drone_phase = ?, \
                is_stale = 0 \
             WHERE id = ?",
        )
        .bind(&now)
        .bind(f64::from(t.battery_pct))
        .bind(f64::from(t.paint_remaining_ml))
        .bind(t.position.lat)
        .bind(t.position.lon)
        .bind(f64::from(t.position.alt_m))
        .bind(drone_phase_str(t.drone_phase))
        .bind(id)
        .execute(self.pool())
        .await?;
        Ok(())
    }

    pub async fn list_drones(&self) -> Result<Vec<DroneRow>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT id, legion_version, capabilities, last_known_battery_pct, \
                    last_known_paint_ml, last_known_position_lat, last_known_position_lon, \
                    last_known_position_alt_m, last_known_drone_phase, is_stale \
             FROM drones ORDER BY id",
        )
        .fetch_all(self.pool())
        .await?;

        rows.into_iter()
            .map(|r| {
                Ok(DroneRow {
                    id: r.get("id"),
                    legion_version: r.get("legion_version"),
                    capabilities: r
                        .try_get::<Option<String>, _>("capabilities")?
                        .and_then(|s| serde_json::from_str(&s).ok())
                        .unwrap_or_default(),
                    battery_pct: r.try_get("last_known_battery_pct").ok(),
                    paint_ml: r.try_get("last_known_paint_ml").ok(),
                    position_lat: r.try_get("last_known_position_lat").ok(),
                    position_lon: r.try_get("last_known_position_lon").ok(),
                    position_alt_m: r.try_get("last_known_position_alt_m").ok(),
                    drone_phase: r.try_get::<Option<String>, _>("last_known_drone_phase").ok().flatten(),
                    is_stale: r.try_get::<i64, _>("is_stale").unwrap_or(0) != 0,
                })
            })
            .collect()
    }

    /// Build a `FleetSnapshot` from the current drones table contents. Used
    /// by the slicer entry point.
    pub async fn fleet_snapshot(&self) -> Result<FleetSnapshot, sqlx::Error> {
        let drones = self
            .list_drones()
            .await?
            .into_iter()
            .map(|r| Drone {
                id: r.id,
                legion_version: r.legion_version,
                capabilities: r.capabilities,
                state: DroneState {
                    battery_pct: r.battery_pct.unwrap_or(0.0) as f32,
                    paint_remaining_ml: r.paint_ml.unwrap_or(0.0) as f32,
                    voltage: 0.0,
                    position: hivemind_protocol::Position {
                        lat: r.position_lat.unwrap_or(0.0),
                        lon: r.position_lon.unwrap_or(0.0),
                        alt_m: r.position_alt_m.unwrap_or(0.0) as f32,
                    },
                    attitude: hivemind_protocol::Attitude::default(),
                    gps_fix: hivemind_protocol::GpsFixType::default(),
                    phase: parse_drone_phase(r.drone_phase.as_deref()),
                    tof_distance_cm: None,
                },
                is_stale: r.is_stale,
            })
            .collect();
        Ok(FleetSnapshot::now(drones))
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct DroneRow {
    pub id: String,
    pub legion_version: Option<String>,
    pub capabilities: Vec<String>,
    pub battery_pct: Option<f64>,
    pub paint_ml: Option<f64>,
    pub position_lat: Option<f64>,
    pub position_lon: Option<f64>,
    pub position_alt_m: Option<f64>,
    pub drone_phase: Option<String>,
    pub is_stale: bool,
}

fn drone_phase_str(p: hivemind_protocol::DronePhase) -> &'static str {
    use hivemind_protocol::DronePhase;
    match p {
        DronePhase::Idle => "Idle",
        DronePhase::Armed => "Armed",
        DronePhase::InAir => "InAir",
        DronePhase::ExecutingStep => "ExecutingStep",
        DronePhase::Holding => "Holding",
        DronePhase::Landing => "Landing",
    }
}

fn parse_drone_phase(s: Option<&str>) -> hivemind_protocol::DronePhase {
    use hivemind_protocol::DronePhase;
    match s {
        Some("Idle") => DronePhase::Idle,
        Some("Armed") => DronePhase::Armed,
        Some("InAir") => DronePhase::InAir,
        Some("ExecutingStep") => DronePhase::ExecutingStep,
        Some("Holding") => DronePhase::Holding,
        Some("Landing") => DronePhase::Landing,
        _ => DronePhase::default(),
    }
}

fn rfc3339_now() -> String {
    OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}
