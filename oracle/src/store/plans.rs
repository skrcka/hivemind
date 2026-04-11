//! Plan persistence — `plans`, `plan_warnings`, `plan_errors`.

use sha2::{Digest, Sha256};
use sqlx::Row;
use time::OffsetDateTime;

use crate::domain::plan::{HivemindPlan, PlanId, PlanStatus};

use super::Store;

impl Store {
    /// Insert a freshly-sliced plan in `Proposed` status. Computes the
    /// body hash and persists summary metrics alongside the JSON body.
    pub async fn insert_plan(&self, plan: &HivemindPlan) -> Result<String, sqlx::Error> {
        let body = serde_json::to_string(plan).unwrap_or_else(|_| "{}".into());
        let body_hash = sha256_hex(&body);
        let snapshot = serde_json::to_string(&plan.fleet_snapshot).unwrap_or_else(|_| "{}".into());

        let plan_id = plan.id.to_string();
        let intent_id = plan.intent.scan.id.clone();

        let created_at = plan
            .created_at
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default();

        let mut tx = self.pool().begin().await?;

        sqlx::query(
            "INSERT INTO plans (
                id, intent_id, status, created_at, proposed_at,
                body_hash, body, fleet_snapshot,
                coverage_total_area_m2, coverage_overlap_pct,
                schedule_total_duration_s, schedule_peak_concurrent_drones,
                resources_paint_ml, resources_battery_cycles, resources_total_flight_time_s
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&plan_id)
        .bind(&intent_id)
        .bind(plan.status.as_str())
        .bind(&created_at)
        .bind(&created_at)
        .bind(&body_hash)
        .bind(&body)
        .bind(&snapshot)
        .bind(plan.coverage.total_area_m2)
        .bind(f64::from(plan.coverage.overlap_pct))
        .bind(i64::try_from(plan.schedule.total_duration_s).unwrap_or(i64::MAX))
        .bind(i64::from(plan.schedule.peak_concurrent_drones))
        .bind(plan.resources.paint_ml)
        .bind(i64::from(plan.resources.battery_cycles))
        .bind(i64::try_from(plan.resources.total_flight_time_s).unwrap_or(i64::MAX))
        .execute(&mut *tx)
        .await?;

        for w in &plan.warnings {
            sqlx::query(
                "INSERT INTO plan_warnings (plan_id, severity, code, message) VALUES (?, ?, ?, ?)",
            )
            .bind(&plan_id)
            .bind(w.severity.as_str())
            .bind(w.code.as_str())
            .bind(&w.message)
            .execute(&mut *tx)
            .await?;
        }

        for e in &plan.errors {
            sqlx::query(
                "INSERT INTO plan_errors (plan_id, code, message, context) VALUES (?, ?, ?, ?)",
            )
            .bind(&plan_id)
            .bind(e.code.as_str())
            .bind(&e.message)
            .bind(e.region_id.as_ref())
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(body_hash)
    }

    /// Fetch the full HivemindPlan JSON body for the given id.
    pub async fn get_plan(&self, plan_id: PlanId) -> Result<Option<HivemindPlan>, sqlx::Error> {
        let id_str = plan_id.to_string();
        let row: Option<(String,)> = sqlx::query_as("SELECT body FROM plans WHERE id = ?")
            .bind(&id_str)
            .fetch_optional(self.pool())
            .await?;
        Ok(row.and_then(|(body,)| serde_json::from_str(&body).ok()))
    }

    /// Fetch the body hash and current status for an existing plan.
    pub async fn plan_summary(
        &self,
        plan_id: PlanId,
    ) -> Result<Option<(PlanStatus, String)>, sqlx::Error> {
        let id_str = plan_id.to_string();
        let row = sqlx::query("SELECT status, body_hash FROM plans WHERE id = ?")
            .bind(&id_str)
            .fetch_optional(self.pool())
            .await?;
        let Some(row) = row else { return Ok(None) };
        let status_str: String = row.get("status");
        let hash: String = row.get("body_hash");
        let status = status_str
            .parse::<PlanStatus>()
            .map_err(|e| sqlx::Error::ColumnDecode {
                index: "status".into(),
                source: Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, e)),
            })?;
        Ok(Some((status, hash)))
    }

    /// Set a plan's status, optionally recording approval metadata.
    pub async fn set_plan_status(
        &self,
        plan_id: PlanId,
        status: PlanStatus,
        approved_by: Option<&str>,
    ) -> Result<(), sqlx::Error> {
        let id_str = plan_id.to_string();
        let now = OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default();

        match status {
            PlanStatus::Approved => {
                sqlx::query(
                    "UPDATE plans SET status = ?, approved_at = ?, approved_by = ? WHERE id = ?",
                )
                .bind(status.as_str())
                .bind(&now)
                .bind(approved_by)
                .bind(&id_str)
                .execute(self.pool())
                .await?;
            }
            PlanStatus::Executing => {
                sqlx::query("UPDATE plans SET status = ?, started_at = ? WHERE id = ?")
                    .bind(status.as_str())
                    .bind(&now)
                    .bind(&id_str)
                    .execute(self.pool())
                    .await?;
            }
            PlanStatus::Complete | PlanStatus::Failed | PlanStatus::Aborted => {
                sqlx::query("UPDATE plans SET status = ?, completed_at = ? WHERE id = ?")
                    .bind(status.as_str())
                    .bind(&now)
                    .bind(&id_str)
                    .execute(self.pool())
                    .await?;
            }
            _ => {
                sqlx::query("UPDATE plans SET status = ? WHERE id = ?")
                    .bind(status.as_str())
                    .bind(&id_str)
                    .execute(self.pool())
                    .await?;
            }
        }
        Ok(())
    }

    /// List plans, optionally filtered by status, ordered by created_at desc.
    pub async fn list_plans(
        &self,
        status: Option<PlanStatus>,
        limit: i64,
    ) -> Result<Vec<PlanListEntry>, sqlx::Error> {
        let rows = if let Some(s) = status {
            sqlx::query(
                "SELECT id, intent_id, status, created_at FROM plans \
                 WHERE status = ? ORDER BY created_at DESC LIMIT ?",
            )
            .bind(s.as_str())
            .bind(limit)
            .fetch_all(self.pool())
            .await?
        } else {
            sqlx::query(
                "SELECT id, intent_id, status, created_at FROM plans \
                 ORDER BY created_at DESC LIMIT ?",
            )
            .bind(limit)
            .fetch_all(self.pool())
            .await?
        };

        rows.into_iter()
            .map(|r| {
                let id_str: String = r.get("id");
                let intent_id: String = r.get("intent_id");
                let status_str: String = r.get("status");
                let created_at: String = r.get("created_at");
                let id: PlanId =
                    id_str.parse().map_err(|e: uuid::Error| sqlx::Error::ColumnDecode {
                        index: "id".into(),
                        source: Box::new(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            e.to_string(),
                        )),
                    })?;
                let status =
                    status_str
                        .parse::<PlanStatus>()
                        .map_err(|e| sqlx::Error::ColumnDecode {
                            index: "status".into(),
                            source: Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, e)),
                        })?;
                Ok(PlanListEntry {
                    id,
                    intent_id,
                    status,
                    created_at,
                })
            })
            .collect()
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct PlanListEntry {
    pub id: PlanId,
    pub intent_id: String,
    pub status: PlanStatus,
    pub created_at: String,
}

fn sha256_hex(s: &str) -> String {
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    hex::encode(h.finalize())
}
