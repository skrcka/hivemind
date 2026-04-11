//! JSON-file-backed `SortieStore`. One file per sortie
//! (`<sortie_id>.json`), one sibling file per progress checkpoint
//! (`<sortie_id>.progress.json`), and a marker file for completed
//! sorties (`<sortie_id>.done`).
//!
//! Writes are atomic: `tempfile::NamedTempFile` in the same directory,
//! followed by `persist()` (which `rename()`s into place on POSIX).

use std::path::{Path, PathBuf};

use hivemind_protocol::Sortie;
use legion_core::error::StoreError;
use legion_core::traits::store::SortieProgress;
use legion_core::SortieStore;
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;

pub struct FileSortieStore {
    root: PathBuf,
}

impl FileSortieStore {
    /// Create a new store rooted at `dir`. The directory is created on
    /// demand if it doesn't already exist.
    pub fn new(dir: impl Into<PathBuf>) -> Result<Self, StoreError> {
        let root = dir.into();
        std::fs::create_dir_all(&root).map_err(|e| StoreError::Io {
            detail: e.to_string(),
        })?;
        Ok(Self { root })
    }

    fn sortie_path(&self, id: &str) -> PathBuf {
        self.root.join(format!("{id}.json"))
    }

    fn progress_path(&self, id: &str) -> PathBuf {
        self.root.join(format!("{id}.progress.json"))
    }

    fn done_path(&self, id: &str) -> PathBuf {
        self.root.join(format!("{id}.done"))
    }

    /// Scan the store for any sortie with a progress checkpoint but no
    /// `.done` marker. Used on boot to build the initial `Hello`.
    pub fn find_in_progress(&self) -> Result<Option<SortieProgress>, StoreError> {
        let entries = std::fs::read_dir(&self.root).map_err(|e| StoreError::Io {
            detail: e.to_string(),
        })?;
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let Some(sortie_id) = name.strip_suffix(".progress.json") else {
                continue;
            };
            if self.done_path(sortie_id).exists() {
                continue;
            }
            let body = std::fs::read_to_string(&path).map_err(|e| StoreError::Io {
                detail: e.to_string(),
            })?;
            let on_disk: OnDiskProgress =
                serde_json::from_str(&body).map_err(|e| StoreError::Corrupt {
                    detail: e.to_string(),
                })?;
            return Ok(Some(on_disk.into_core()));
        }
        Ok(None)
    }
}

/// On-disk shape for progress. Has an extra `v` field for schema
/// evolution down the line.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct OnDiskProgress {
    v: u32,
    sortie_id: String,
    last_completed_step: Option<u32>,
    checkpoint_ms: u64,
}

impl OnDiskProgress {
    fn from_core(p: &SortieProgress) -> Self {
        Self {
            v: 1,
            sortie_id: p.sortie_id.clone(),
            last_completed_step: p.last_completed_step,
            checkpoint_ms: p.checkpoint_ms,
        }
    }
    fn into_core(self) -> SortieProgress {
        SortieProgress {
            sortie_id: self.sortie_id,
            last_completed_step: self.last_completed_step,
            checkpoint_ms: self.checkpoint_ms,
        }
    }
}

fn atomic_write(path: &Path, contents: &[u8]) -> Result<(), StoreError> {
    let parent = path.parent().ok_or_else(|| StoreError::Io {
        detail: "no parent directory".into(),
    })?;
    let tmp = NamedTempFile::new_in(parent).map_err(|e| StoreError::Io {
        detail: e.to_string(),
    })?;
    std::fs::write(tmp.path(), contents).map_err(|e| StoreError::Io {
        detail: e.to_string(),
    })?;
    tmp.persist(path).map_err(|e| StoreError::Io {
        detail: e.to_string(),
    })?;
    Ok(())
}

impl SortieStore for FileSortieStore {
    async fn put(&self, sortie: &Sortie) -> Result<(), StoreError> {
        let body = serde_json::to_vec_pretty(sortie).map_err(|e| StoreError::Io {
            detail: e.to_string(),
        })?;
        let path = self.sortie_path(&sortie.sortie_id);
        tokio::task::block_in_place(|| atomic_write(&path, &body))
    }

    async fn get(&self, sortie_id: &str) -> Result<Sortie, StoreError> {
        let path = self.sortie_path(sortie_id);
        let body = std::fs::read_to_string(&path).map_err(|e| StoreError::NotFound {
            sortie_id: format!("{sortie_id}: {e}"),
        })?;
        serde_json::from_str(&body).map_err(|e| StoreError::Corrupt {
            detail: e.to_string(),
        })
    }

    async fn checkpoint(&self, progress: &SortieProgress) -> Result<(), StoreError> {
        let on_disk = OnDiskProgress::from_core(progress);
        let body = serde_json::to_vec_pretty(&on_disk).map_err(|e| StoreError::Io {
            detail: e.to_string(),
        })?;
        let path = self.progress_path(&progress.sortie_id);
        tokio::task::block_in_place(|| atomic_write(&path, &body))
    }

    async fn load_progress(
        &self,
        sortie_id: &str,
    ) -> Result<Option<SortieProgress>, StoreError> {
        let path = self.progress_path(sortie_id);
        if !path.exists() {
            return Ok(None);
        }
        let body = std::fs::read_to_string(&path).map_err(|e| StoreError::Io {
            detail: e.to_string(),
        })?;
        let on_disk: OnDiskProgress =
            serde_json::from_str(&body).map_err(|e| StoreError::Corrupt {
                detail: e.to_string(),
            })?;
        Ok(Some(on_disk.into_core()))
    }

    async fn list(&self) -> Result<Vec<String>, StoreError> {
        let mut ids = Vec::new();
        let entries = std::fs::read_dir(&self.root).map_err(|e| StoreError::Io {
            detail: e.to_string(),
        })?;
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if let Some(id) = name.strip_suffix(".json") {
                    if !id.ends_with(".progress") {
                        ids.push(id.to_string());
                    }
                }
            }
        }
        Ok(ids)
    }

    async fn mark_complete(&self, sortie_id: &str) -> Result<(), StoreError> {
        let path = self.done_path(sortie_id);
        tokio::task::block_in_place(|| atomic_write(&path, b""))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hivemind_protocol::{
        RadioLossBehaviour, RadioLossPolicy, SortieStep, StepType, Waypoint,
    };

    fn sample_sortie(id: &str) -> Sortie {
        Sortie {
            sortie_id: id.into(),
            plan_id: "plan-1".into(),
            drone_id: "drone-01".into(),
            paint_volume_ml: 100.0,
            expected_duration_s: 30,
            steps: vec![SortieStep {
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
            }],
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn put_get_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let store = FileSortieStore::new(tmp.path()).unwrap();
        let s = sample_sortie("sortie-rt");
        store.put(&s).await.unwrap();
        let got = store.get("sortie-rt").await.unwrap();
        assert_eq!(got.sortie_id, s.sortie_id);
        assert_eq!(got.steps.len(), s.steps.len());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn checkpoint_and_load_progress() {
        let tmp = tempfile::tempdir().unwrap();
        let store = FileSortieStore::new(tmp.path()).unwrap();
        let s = sample_sortie("sortie-cp");
        store.put(&s).await.unwrap();
        store
            .checkpoint(&SortieProgress {
                sortie_id: "sortie-cp".into(),
                last_completed_step: Some(2),
                checkpoint_ms: 1234,
            })
            .await
            .unwrap();

        let p = store.load_progress("sortie-cp").await.unwrap().unwrap();
        assert_eq!(p.last_completed_step, Some(2));
        assert_eq!(p.checkpoint_ms, 1234);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn find_in_progress_ignores_completed() {
        let tmp = tempfile::tempdir().unwrap();
        let store = FileSortieStore::new(tmp.path()).unwrap();
        store.put(&sample_sortie("a")).await.unwrap();
        store
            .checkpoint(&SortieProgress {
                sortie_id: "a".into(),
                last_completed_step: Some(0),
                checkpoint_ms: 10,
            })
            .await
            .unwrap();
        store.mark_complete("a").await.unwrap();

        assert!(store.find_in_progress().unwrap().is_none());

        store.put(&sample_sortie("b")).await.unwrap();
        store
            .checkpoint(&SortieProgress {
                sortie_id: "b".into(),
                last_completed_step: Some(0),
                checkpoint_ms: 20,
            })
            .await
            .unwrap();

        let found = store.find_in_progress().unwrap().unwrap();
        assert_eq!(found.sortie_id, "b");
    }
}
