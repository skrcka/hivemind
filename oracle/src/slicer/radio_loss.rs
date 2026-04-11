//! Stamp default `RadioLossPolicy` onto every step in a sortie based on its
//! `StepType`. The defaults are deliberately conservative; future versions
//! will allow per-region overrides driven by `intent.constraints`.
//!
//! See [legion's policy table](../../../../legion/README.md#the-sortie) for
//! the rationale behind each default.

use hivemind_protocol::{RadioLossBehaviour, RadioLossPolicy, Sortie, StepType};

use crate::config::SlicerConfig;

pub fn assign_defaults(mut sortie: Sortie, _cfg: &SlicerConfig) -> Sortie {
    for step in &mut sortie.steps {
        step.radio_loss = default_for(step.step_type);
    }
    sortie
}

fn default_for(step_type: StepType) -> RadioLossPolicy {
    match step_type {
        StepType::Takeoff => RadioLossPolicy {
            behaviour: RadioLossBehaviour::HoldThenRtl,
            silent_timeout_s: 5.0,
            hold_then_rtl_after_s: Some(10.0),
        },
        StepType::SprayPass => RadioLossPolicy {
            behaviour: RadioLossBehaviour::Continue,
            silent_timeout_s: 60.0,
            hold_then_rtl_after_s: None,
        },
        StepType::RefillApproach => RadioLossPolicy {
            behaviour: RadioLossBehaviour::HoldThenRtl,
            silent_timeout_s: 15.0,
            hold_then_rtl_after_s: Some(30.0),
        },
        StepType::RefillWait => RadioLossPolicy {
            behaviour: RadioLossBehaviour::HoldThenRtl,
            silent_timeout_s: 60.0,
            hold_then_rtl_after_s: Some(60.0),
        },
        StepType::Transit | StepType::ReturnToBase | StepType::Land => RadioLossPolicy {
            behaviour: RadioLossBehaviour::Continue,
            silent_timeout_s: 30.0,
            hold_then_rtl_after_s: None,
        },
    }
}
