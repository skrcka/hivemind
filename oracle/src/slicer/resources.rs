//! Resource estimation — paint volume, battery cycles, total flight time.

use hivemind_protocol::Sortie;

use crate::config::SlicerConfig;
use crate::domain::plan::ResourceEstimate;

pub fn estimate(sorties: &[Sortie], _cfg: &SlicerConfig) -> ResourceEstimate {
    let paint_ml: f64 = sorties.iter().map(|s| f64::from(s.paint_volume_ml)).sum();
    let total_flight_time_s: u64 = sorties.iter().map(|s| u64::from(s.expected_duration_s)).sum();

    // v1: assume one battery per sortie. v2 will model battery state over
    // time including refill cycles.
    let battery_cycles = u32::try_from(sorties.len()).unwrap_or(u32::MAX);

    ResourceEstimate {
        paint_ml,
        battery_cycles,
        total_flight_time_s,
    }
}
