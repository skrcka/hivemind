//! Wall-clock scheduling — sum durations across sorties to compute the
//! `FleetSchedule` summary.

use hivemind_protocol::Sortie;

use crate::domain::plan::FleetSchedule;

pub fn build(sorties: &[Sortie]) -> FleetSchedule {
    let total_sorties = u32::try_from(sorties.len()).unwrap_or(u32::MAX);
    let total_duration_s: u64 = sorties.iter().map(|s| u64::from(s.expected_duration_s)).sum();

    // For v1 with one drone the peak concurrent count is just the number of
    // distinct drones across all sorties (≤ fleet size). For v2 this becomes
    // the actual schedule overlap.
    let peak_concurrent_drones = u32::from(!sorties.is_empty());

    FleetSchedule {
        total_sorties,
        total_duration_s,
        peak_concurrent_drones,
        refill_cycles: 0,
    }
}
