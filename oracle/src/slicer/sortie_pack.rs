//! Sortie packing — bundle spray passes into per-drone sorties.
//!
//! v1 strategy: distribute passes across drones in **contiguous chunks**
//! (not round-robin) so adjacent drones work on adjacent strips of the
//! surface. This is the natural input to the lane-assignment stage that
//! will land in v1+ — each chunk is a candidate "lane" along v.
//!
//! Each drone gets exactly one sortie containing its assigned passes. v2
//! will refine this with per-drone battery + paint capacity, splitting one
//! drone's work across multiple sorties when needed.

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
    drones: &[Drone],
    passes: &[SprayPass],
    _cfg: &SlicerConfig,
) -> Vec<RawSortie> {
    if passes.is_empty() || drones.is_empty() {
        return Vec::new();
    }

    let drone_count = drones.len();
    let pass_count = passes.len();

    // Contiguous-chunk distribution: drone i gets passes [i*base + extra_i,
    // (i+1)*base + extra_{i+1}) where the first `remainder` drones get one
    // extra pass each. This keeps adjacent passes on the same drone, which
    // matters because spatial locality ⇒ shorter ferry distances.
    let base = pass_count / drone_count;
    let remainder = pass_count % drone_count;

    let mut sorties = Vec::with_capacity(drone_count);
    let mut cursor = 0usize;
    for (i, drone) in drones.iter().enumerate() {
        let take = base + usize::from(i < remainder);
        if take == 0 {
            // More drones than passes — leftover drones get nothing.
            break;
        }
        let chunk = passes[cursor..cursor + take].to_vec();
        cursor += take;

        sorties.push(RawSortie {
            sortie_id: format!("sortie-{}", Uuid::now_v7()),
            plan_id,
            drone_id: drone.id.clone(),
            sortie_index: u32::try_from(i).unwrap_or(0),
            passes: chunk,
        });
    }

    sorties
}
