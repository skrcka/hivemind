//! Validate the assembled plan and emit warnings. Errors are produced
//! upstream (in coverage/sortie_pack); this stage only generates non-fatal
//! warnings the operator should see in the proposal.

use crate::config::SlicerConfig;
use crate::domain::plan::{
    FleetSchedule, PlanWarning, PlanWarningCode, PlanWarningSeverity, ResourceEstimate,
};

const LONG_DURATION_THRESHOLD_S: u64 = 4 * 60 * 60; // 4 hours

pub fn run(
    schedule: &FleetSchedule,
    _resources: &ResourceEstimate,
    _cfg: &SlicerConfig,
) -> Vec<PlanWarning> {
    let mut warnings = Vec::new();

    if schedule.total_duration_s > LONG_DURATION_THRESHOLD_S {
        #[allow(clippy::cast_precision_loss)]
        let hours = schedule.total_duration_s as f64 / 3600.0;
        warnings.push(PlanWarning {
            severity: PlanWarningSeverity::Warn,
            code: PlanWarningCode::LongDuration,
            message: format!(
                "plan duration ({hours:.1} h) exceeds {} h threshold; consider splitting",
                LONG_DURATION_THRESHOLD_S / 3600
            ),
        });
    }

    warnings
}
