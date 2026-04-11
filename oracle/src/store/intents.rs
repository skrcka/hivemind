//! Intent persistence — `intents` and `intent_regions` tables.

use sqlx::{Pool, Sqlite};
use time::OffsetDateTime;

use crate::domain::intent::Intent;

use super::Store;

impl Store {
    /// Insert an intent and its regions in a single transaction.
    /// Returns the intent's `id` (the scan id from `intent.scan.id`).
    pub async fn insert_intent(&self, intent: &Intent) -> Result<String, sqlx::Error> {
        let id = intent.scan.id.clone();
        let mut tx = self.pool().begin().await?;

        let now = OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default();
        let constraints = serde_json::to_string(&intent.constraints).unwrap_or_else(|_| "{}".into());
        let georef: i64 = i64::from(intent.scan.georeferenced);
        let source_file = intent.scan.source_file.clone();

        sqlx::query(
            "INSERT INTO intents (id, received_at, source_file, georeferenced, constraints) \
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(&now)
        .bind(&source_file)
        .bind(georef)
        .bind(&constraints)
        .execute(&mut *tx)
        .await?;

        for region in &intent.regions {
            let faces_json = serde_json::to_string(&region.faces).unwrap_or_else(|_| "[]".into());
            let face_count = i64::try_from(region.faces.len()).unwrap_or(i64::MAX);
            let paint_spec = region
                .paint_spec
                .as_ref()
                .map(|s| serde_json::to_string(s).unwrap_or_else(|_| "{}".into()));
            sqlx::query(
                "INSERT INTO intent_regions (intent_id, id, name, area_m2, face_count, paint_spec, faces) \
                 VALUES (?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(&id)
            .bind(&region.id)
            .bind(&region.name)
            .bind(region.area_m2)
            .bind(face_count)
            .bind(&paint_spec)
            .bind(&faces_json)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(id)
    }

    /// Load an intent by id, or return `None` if not found.
    pub async fn get_intent(&self, id: &str) -> Result<Option<Intent>, sqlx::Error> {
        load_intent(self.pool(), id).await
    }
}

async fn load_intent(pool: &Pool<Sqlite>, id: &str) -> Result<Option<Intent>, sqlx::Error> {
    use crate::domain::intent::{Face, MeshRegion, OperatorConstraints, PaintSpec, ScanRef};

    let row: Option<(String, Option<String>, i64, String)> = sqlx::query_as(
        "SELECT id, source_file, georeferenced, constraints FROM intents WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    let Some((scan_id, source_file, georef, constraints_str)) = row else {
        return Ok(None);
    };

    let region_rows: Vec<(String, String, f64, Option<String>, String)> = sqlx::query_as(
        "SELECT id, name, area_m2, paint_spec, faces FROM intent_regions WHERE intent_id = ?",
    )
    .bind(id)
    .fetch_all(pool)
    .await?;

    let regions = region_rows
        .into_iter()
        .map(|(rid, name, area_m2, paint_spec_json, faces_json)| {
            let faces: Vec<Face> = serde_json::from_str(&faces_json).unwrap_or_default();
            let paint_spec = paint_spec_json
                .as_deref()
                .and_then(|s| serde_json::from_str::<PaintSpec>(s).ok());
            MeshRegion {
                id: rid,
                name,
                faces,
                area_m2,
                paint_spec,
            }
        })
        .collect();

    let constraints: OperatorConstraints =
        serde_json::from_str(&constraints_str).unwrap_or_default();

    Ok(Some(Intent {
        version: "1.0".into(),
        scan: ScanRef {
            id: scan_id,
            source_file,
            georeferenced: georef != 0,
        },
        regions,
        constraints,
    }))
}
