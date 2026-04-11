//! Audit log persistence — append-only record of every command, approval,
//! and gate decision.

use serde::Serialize;
use serde_json::Value;
use sqlx::Row;
use time::OffsetDateTime;

use super::Store;

#[derive(Debug, Clone)]
pub struct AuditEntry {
    pub actor: String,
    pub event: String,
    pub plan_id: Option<String>,
    pub sortie_id: Option<String>,
    pub drone_id: Option<String>,
    pub payload: Value,
}

impl Store {
    pub async fn audit(&self, entry: AuditEntry) -> Result<(), sqlx::Error> {
        let now = OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default();
        let payload_str = serde_json::to_string(&entry.payload).unwrap_or_else(|_| "{}".into());
        sqlx::query(
            "INSERT INTO audit_log (ts, actor, event, plan_id, sortie_id, drone_id, payload) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&now)
        .bind(&entry.actor)
        .bind(&entry.event)
        .bind(&entry.plan_id)
        .bind(&entry.sortie_id)
        .bind(&entry.drone_id)
        .bind(&payload_str)
        .execute(self.pool())
        .await?;
        Ok(())
    }

    pub async fn list_audit(&self, since: Option<&str>, limit: i64) -> Result<Vec<AuditRow>, sqlx::Error> {
        let rows = if let Some(s) = since {
            sqlx::query(
                "SELECT id, ts, actor, event, plan_id, sortie_id, drone_id, payload \
                 FROM audit_log WHERE ts >= ? ORDER BY id DESC LIMIT ?",
            )
            .bind(s)
            .bind(limit)
            .fetch_all(self.pool())
            .await?
        } else {
            sqlx::query(
                "SELECT id, ts, actor, event, plan_id, sortie_id, drone_id, payload \
                 FROM audit_log ORDER BY id DESC LIMIT ?",
            )
            .bind(limit)
            .fetch_all(self.pool())
            .await?
        };
        rows.into_iter()
            .map(|r| {
                Ok(AuditRow {
                    id: r.get("id"),
                    ts: r.get("ts"),
                    actor: r.get("actor"),
                    event: r.get("event"),
                    plan_id: r.get("plan_id"),
                    sortie_id: r.get("sortie_id"),
                    drone_id: r.get("drone_id"),
                    payload: r
                        .try_get::<String, _>("payload")
                        .ok()
                        .and_then(|s| serde_json::from_str(&s).ok())
                        .unwrap_or(Value::Null),
                })
            })
            .collect()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AuditRow {
    pub id: i64,
    pub ts: String,
    pub actor: String,
    pub event: String,
    pub plan_id: Option<String>,
    pub sortie_id: Option<String>,
    pub drone_id: Option<String>,
    pub payload: Value,
}
