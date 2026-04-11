//! Wall-clock scheduling — sum durations across sorties to compute the
//! `FleetSchedule` summary.

use std::collections::HashSet;

use hivemind_protocol::Sortie;

use crate::domain::plan::FleetSchedule;

pub fn build(sorties: &[Sortie]) -> FleetSchedule {
    let total_sorties = u32::try_from(sorties.len()).unwrap_or(u32::MAX);

    // v1 sorties run in parallel (one per drone) so the wall-clock duration
    // is the longest sortie, not the sum.
    let total_duration_s: u64 = sorties
        .iter()
        .map(|s| u64::from(s.expected_duration_s))
        .max()
        .unwrap_or(0);

    // Distinct drones touched by any sortie. v1 = one sortie per drone, so
    // this equals `sorties.len()`. v2 with multi-sortie-per-drone packing
    // will pull this from the actual schedule overlap.
    let peak_concurrent_drones = u32::try_from(
        sorties
            .iter()
            .map(|s| s.drone_id.as_str())
            .collect::<HashSet<_>>()
            .len(),
    )
    .unwrap_or(u32::MAX);

    FleetSchedule {
        total_sorties,
        total_duration_s,
        peak_concurrent_drones,
        refill_cycles: 0,
    }
}
