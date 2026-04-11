//! Sortie packing — bundle spray passes into per-drone sorties honouring
//! battery and paint capacity. v1 puts everything in one sortie for the
//! single available drone; v2 will iterate the per-drone capacity model.

use hivemind_protocol::SortieId;
use uuid::Uuid;

use crate::config::SlicerConfig;
use crate::domain::{fleet::Drone, plan::PlanId};

use super::coverage::SprayPass;

/// A "raw" sortie — just the spray passes assigned to one drone, before the
/// step-assembly stage wraps them in a typed `Takeoff → Transit → SprayPass …`
/// sequence.
#[derive(Debug, Clone)]
pub struct RawSortie {
    pub sortie_id: SortieId,
    pub plan_id: PlanId,
    pub drone_id: String,
    pub sortie_index: u32,
    pub passes: Vec<SprayPass>,
}

pub fn pack(
    plan_id: PlanId,
    drone: &Drone,
    passes: &[SprayPass],
    _cfg: &SlicerConfig,
) -> Vec<RawSortie> {
    if passes.is_empty() {
        return Vec::new();
    }

    // v1: single sortie containing all passes.
    let sortie = RawSortie {
        sortie_id: format!("sortie-{}", Uuid::now_v7()),
        plan_id,
        drone_id: drone.id.clone(),
        sortie_index: 0,
        passes: passes.to_vec(),
    };

    vec![sortie]
}
