//! Sortie + sortie_step persistence. Mirrors a `hivemind_protocol::Sortie`
//! into the relational tables so the API can query progress, and so the
//! Apply Supervisor can resume after a crash.
//!
//! SQLite stores REAL as f64 but the wire types use f32 throughout, so
//! every f64→f32 cast at the load boundary is intentional.

#![allow(clippy::cast_possible_truncation)]

use hivemind_protocol::{RadioLossBehaviour, Sortie, SortieStep, StepType, Waypoint};
use sqlx::Row;
use time::OffsetDateTime;

use crate::domain::plan::PlanId;

use super::Store;

impl Store {
    /// Insert every sortie in a plan in `Pending` status, plus all their
    /// steps. Called by the API right after `set_plan_status(Approved)`.
    pub async fn insert_plan_sorties(
        &self,
        plan_id: PlanId,
        sorties: &[Sortie],
    ) -> Result<(), sqlx::Error> {
        let plan_id_str = plan_id.to_string();
        let mut tx = self.pool().begin().await?;
        for (idx, sortie) in sorties.iter().enumerate() {
            sqlx::query(
                "INSERT INTO sorties (id, plan_id, drone_id, sortie_index, status, paint_volume_ml, expected_duration_s) \
                 VALUES (?, ?, ?, ?, 'Pending', ?, ?)",
            )
            .bind(&sortie.sortie_id)
            .bind(&plan_id_str)
            .bind(&sortie.drone_id)
            .bind(i64::try_from(idx).unwrap_or(i64::MAX))
            .bind(f64::from(sortie.paint_volume_ml))
            .bind(i64::from(sortie.expected_duration_s))
            .execute(&mut *tx)
            .await?;

            for step in &sortie.steps {
                let path_json = step
                    .path
                    .as_ref()
                    .map(|p| serde_json::to_string(p).unwrap_or_else(|_| "[]".into()));
                sqlx::query(
                    "INSERT INTO sortie_steps (
                        sortie_id, step_index, step_type,
                        waypoint_lat, waypoint_lon, waypoint_alt_m, waypoint_yaw_deg,
                        speed_m_s, spray,
                        radio_loss_behaviour, radio_loss_silent_timeout_s, radio_loss_hold_then_rtl_after_s,
                        expected_duration_s, path
                    ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                )
                .bind(&sortie.sortie_id)
                .bind(i64::from(step.index))
                .bind(step_type_str(step.step_type))
                .bind(step.waypoint.lat)
                .bind(step.waypoint.lon)
                .bind(f64::from(step.waypoint.alt_m))
                .bind(step.waypoint.yaw_deg.map(f64::from))
                .bind(f64::from(step.speed_m_s))
                .bind(i64::from(step.spray))
                .bind(behaviour_str(step.radio_loss.behaviour))
                .bind(f64::from(step.radio_loss.silent_timeout_s))
                .bind(step.radio_loss.hold_then_rtl_after_s.map(f64::from))
                .bind(i64::from(step.expected_duration_s))
                .bind(path_json)
                .execute(&mut *tx)
                .await?;
            }
        }
        tx.commit().await?;
        Ok(())
    }

    /// Update a sortie's status, optionally recording lifecycle timestamps.
    pub async fn set_sortie_status(
        &self,
        sortie_id: &str,
        status: SortieStatus,
        failure_reason: Option<&str>,
    ) -> Result<(), sqlx::Error> {
        let now = OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default();
        match status {
            SortieStatus::Uploaded => {
                sqlx::query("UPDATE sorties SET status = 'Uploaded', uploaded_at = ? WHERE id = ?")
                    .bind(&now)
                    .bind(sortie_id)
                    .execute(self.pool())
                    .await?;
            }
            SortieStatus::Executing => {
                sqlx::query("UPDATE sorties SET status = 'Executing', started_at = ? WHERE id = ?")
                    .bind(&now)
                    .bind(sortie_id)
                    .execute(self.pool())
                    .await?;
            }
            SortieStatus::Complete => {
                sqlx::query("UPDATE sorties SET status = 'Complete', ended_at = ? WHERE id = ?")
                    .bind(&now)
                    .bind(sortie_id)
                    .execute(self.pool())
                    .await?;
            }
            SortieStatus::Failed => {
                sqlx::query(
                    "UPDATE sorties SET status = 'Failed', ended_at = ?, failure_reason = ? WHERE id = ?",
                )
                .bind(&now)
                .bind(failure_reason)
                .bind(sortie_id)
                .execute(self.pool())
                .await?;
            }
            SortieStatus::Aborted => {
                sqlx::query(
                    "UPDATE sorties SET status = 'Aborted', ended_at = ?, failure_reason = ? WHERE id = ?",
                )
                .bind(&now)
                .bind(failure_reason)
                .bind(sortie_id)
                .execute(self.pool())
                .await?;
            }
            SortieStatus::Pending => {
                sqlx::query("UPDATE sorties SET status = 'Pending' WHERE id = ?")
                    .bind(sortie_id)
                    .execute(self.pool())
                    .await?;
            }
        }
        Ok(())
    }

    /// Reload a sortie + its steps. Used when an Apply Supervisor restarts
    /// and needs the typed `Sortie` shape to drive the handshake.
    pub async fn load_sortie(&self, sortie_id: &str) -> Result<Option<Sortie>, sqlx::Error> {
        let row = sqlx::query(
            "SELECT plan_id, drone_id, paint_volume_ml, expected_duration_s FROM sorties WHERE id = ?",
        )
        .bind(sortie_id)
        .fetch_optional(self.pool())
        .await?;
        let Some(row) = row else { return Ok(None) };

        let plan_id: String = row.get("plan_id");
        let drone_id: String = row.get("drone_id");
        let paint_volume_ml: f64 = row.get("paint_volume_ml");
        let expected_duration_s: i64 = row.get("expected_duration_s");

        let step_rows = sqlx::query(
            "SELECT step_index, step_type, waypoint_lat, waypoint_lon, waypoint_alt_m, \
                    waypoint_yaw_deg, speed_m_s, spray, radio_loss_behaviour, \
                    radio_loss_silent_timeout_s, radio_loss_hold_then_rtl_after_s, \
                    expected_duration_s, path \
             FROM sortie_steps WHERE sortie_id = ? ORDER BY step_index",
        )
        .bind(sortie_id)
        .fetch_all(self.pool())
        .await?;

        let mut steps = Vec::with_capacity(step_rows.len());
        for r in step_rows {
            let step_index: i64 = r.get("step_index");
            let step_type: String = r.get("step_type");
            let lat: f64 = r.get("waypoint_lat");
            let lon: f64 = r.get("waypoint_lon");
            let alt_m: f64 = r.get("waypoint_alt_m");
            let yaw_deg: Option<f64> = r.get("waypoint_yaw_deg");
            let speed: f64 = r.get("speed_m_s");
            let spray: i64 = r.get("spray");
            let behaviour: String = r.get("radio_loss_behaviour");
            let silent_timeout: f64 = r.get("radio_loss_silent_timeout_s");
            let hold_then: Option<f64> = r.get("radio_loss_hold_then_rtl_after_s");
            let expected_dur: i64 = r.get("expected_duration_s");
            let path_json: Option<String> = r.get("path");

            let path = path_json
                .as_deref()
                .and_then(|s| serde_json::from_str::<Vec<Waypoint>>(s).ok());

            steps.push(SortieStep {
                index: u32::try_from(step_index).unwrap_or(0),
                step_type: parse_step_type(&step_type)?,
                waypoint: Waypoint {
                    lat,
                    lon,
                    alt_m: alt_m as f32,
                    yaw_deg: yaw_deg.map(|y| y as f32),
                },
                path,
                speed_m_s: speed as f32,
                spray: spray != 0,
                radio_loss: hivemind_protocol::RadioLossPolicy {
                    behaviour: parse_behaviour(&behaviour)?,
                    silent_timeout_s: silent_timeout as f32,
                    hold_then_rtl_after_s: hold_then.map(|v| v as f32),
                },
                expected_duration_s: u32::try_from(expected_dur).unwrap_or(0),
            });
        }

        Ok(Some(Sortie {
            sortie_id: sortie_id.to_string(),
            plan_id,
            drone_id,
            steps,
            paint_volume_ml: paint_volume_ml as f32,
            expected_duration_s: u32::try_from(expected_duration_s).unwrap_or(0),
        }))
    }
}

#[derive(Debug, Clone, Copy)]
pub enum SortieStatus {
    Pending,
    Uploaded,
    Executing,
    Complete,
    Failed,
    Aborted,
}

fn step_type_str(t: StepType) -> &'static str {
    match t {
        StepType::Takeoff => "Takeoff",
        StepType::Transit => "Transit",
        StepType::SprayPass => "SprayPass",
        StepType::RefillApproach => "RefillApproach",
        StepType::RefillWait => "RefillWait",
        StepType::ReturnToBase => "ReturnToBase",
        StepType::Land => "Land",
    }
}

fn parse_step_type(s: &str) -> Result<StepType, sqlx::Error> {
    match s {
        "Takeoff" => Ok(StepType::Takeoff),
        "Transit" => Ok(StepType::Transit),
        "SprayPass" => Ok(StepType::SprayPass),
        "RefillApproach" => Ok(StepType::RefillApproach),
        "RefillWait" => Ok(StepType::RefillWait),
        "ReturnToBase" => Ok(StepType::ReturnToBase),
        "Land" => Ok(StepType::Land),
        other => Err(sqlx::Error::ColumnDecode {
            index: "step_type".into(),
            source: format!("unknown step_type: {other}").into(),
        }),
    }
}

fn behaviour_str(b: RadioLossBehaviour) -> &'static str {
    match b {
        RadioLossBehaviour::Continue => "Continue",
        RadioLossBehaviour::HoldThenRtl => "HoldThenRtl",
        RadioLossBehaviour::RtlImmediately => "RtlImmediately",
    }
}

fn parse_behaviour(s: &str) -> Result<RadioLossBehaviour, sqlx::Error> {
    match s {
        "Continue" => Ok(RadioLossBehaviour::Continue),
        "HoldThenRtl" => Ok(RadioLossBehaviour::HoldThenRtl),
        "RtlImmediately" => Ok(RadioLossBehaviour::RtlImmediately),
        other => Err(sqlx::Error::ColumnDecode {
            index: "radio_loss_behaviour".into(),
            source: format!("unknown radio_loss_behaviour: {other}").into(),
        }),
    }
}
