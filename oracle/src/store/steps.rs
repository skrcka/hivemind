//! Step progress persistence — the resume-after-crash data path.

use sqlx::Row;
use time::OffsetDateTime;

use crate::domain::plan::PlanId;

use super::Store;

#[derive(Debug, Clone, Copy)]
pub enum StepProgressState {
    Gating,
    Running,
    Complete,
    Failed,
    Held,
    Aborted,
}

impl StepProgressState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Gating => "Gating",
            Self::Running => "Running",
            Self::Complete => "Complete",
            Self::Failed => "Failed",
            Self::Held => "Held",
            Self::Aborted => "Aborted",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum GateDecision {
    AutoProceed,
    OperatorRequired,
    FleetConflict,
    AbortSortie,
}

impl GateDecision {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AutoProceed => "AutoProceed",
            Self::OperatorRequired => "OperatorRequired",
            Self::FleetConflict => "FleetConflict",
            Self::AbortSortie => "AbortSortie",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct StepCompletion {
    pub position_lat: f64,
    pub position_lon: f64,
    pub position_alt_m: f32,
    pub battery_pct: f32,
    pub paint_remaining_ml: f32,
    pub duration_s: f32,
}

impl Store {
    /// Mark a step as gating (we've decided to dispatch it).
    pub async fn record_step_gated(
        &self,
        sortie_id: &str,
        step_index: u32,
        decision: GateDecision,
        reason: Option<&str>,
    ) -> Result<(), sqlx::Error> {
        let now = rfc3339_now();
        sqlx::query(
            "INSERT INTO step_progress (sortie_id, step_index, state, gate_decision, gate_reason, gated_at) \
             VALUES (?, ?, 'Gating', ?, ?, ?) \
             ON CONFLICT(sortie_id, step_index) DO UPDATE SET \
                state = 'Gating', gate_decision = excluded.gate_decision, \
                gate_reason = excluded.gate_reason, gated_at = excluded.gated_at",
        )
        .bind(sortie_id)
        .bind(i64::from(step_index))
        .bind(decision.as_str())
        .bind(reason)
        .bind(&now)
        .execute(self.pool())
        .await?;
        Ok(())
    }

    /// Mark a step as running (legion has acknowledged Proceed).
    pub async fn record_step_running(
        &self,
        sortie_id: &str,
        step_index: u32,
    ) -> Result<(), sqlx::Error> {
        let now = rfc3339_now();
        sqlx::query(
            "UPDATE step_progress SET state = 'Running', started_at = ? \
             WHERE sortie_id = ? AND step_index = ?",
        )
        .bind(&now)
        .bind(sortie_id)
        .bind(i64::from(step_index))
        .execute(self.pool())
        .await?;
        Ok(())
    }

    /// Mark a step as complete and persist the telemetry snapshot.
    pub async fn record_step_complete(
        &self,
        sortie_id: &str,
        step_index: u32,
        completion: &StepCompletion,
    ) -> Result<(), sqlx::Error> {
        let now = rfc3339_now();
        sqlx::query(
            "UPDATE step_progress SET state = 'Complete', completed_at = ?, \
                position_lat = ?, position_lon = ?, position_alt_m = ?, \
                battery_pct = ?, paint_remaining_ml = ?, duration_s = ? \
             WHERE sortie_id = ? AND step_index = ?",
        )
        .bind(&now)
        .bind(completion.position_lat)
        .bind(completion.position_lon)
        .bind(f64::from(completion.position_alt_m))
        .bind(f64::from(completion.battery_pct))
        .bind(f64::from(completion.paint_remaining_ml))
        .bind(f64::from(completion.duration_s))
        .bind(sortie_id)
        .bind(i64::from(step_index))
        .execute(self.pool())
        .await?;
        Ok(())
    }

    /// Mark a step as failed.
    pub async fn record_step_failed(
        &self,
        sortie_id: &str,
        step_index: u32,
        reason: &str,
    ) -> Result<(), sqlx::Error> {
        let now = rfc3339_now();
        sqlx::query(
            "UPDATE step_progress SET state = 'Failed', completed_at = ?, failure_reason = ? \
             WHERE sortie_id = ? AND step_index = ?",
        )
        .bind(&now)
        .bind(reason)
        .bind(sortie_id)
        .bind(i64::from(step_index))
        .execute(self.pool())
        .await?;
        Ok(())
    }

    /// Read the highest completed step index for a sortie. Used for
    /// restart-resume.
    pub async fn last_completed_step(
        &self,
        sortie_id: &str,
    ) -> Result<Option<u32>, sqlx::Error> {
        let row = sqlx::query(
            "SELECT MAX(step_index) AS max_step FROM step_progress \
             WHERE sortie_id = ? AND state = 'Complete'",
        )
        .bind(sortie_id)
        .fetch_optional(self.pool())
        .await?;
        Ok(row
            .and_then(|r| r.try_get::<Option<i64>, _>("max_step").ok().flatten())
            .map(|v| u32::try_from(v).unwrap_or(0)))
    }

    /// Used by the restart path to find any plans currently in `Executing`
    /// state and resume them.
    pub async fn executing_plans(&self) -> Result<Vec<PlanId>, sqlx::Error> {
        let rows = sqlx::query("SELECT id FROM plans WHERE status = 'Executing'")
            .fetch_all(self.pool())
            .await?;
        rows.into_iter()
            .map(|r| {
                let id_str: String = r.get("id");
                id_str.parse::<PlanId>().map_err(|e| sqlx::Error::ColumnDecode {
                    index: "id".into(),
                    source: Box::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        e.to_string(),
                    )),
                })
            })
            .collect()
    }
}

fn rfc3339_now() -> String {
    OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}
