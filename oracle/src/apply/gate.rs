//! Step gating — evaluated by the Apply Supervisor before each step
//! transition. v1 returns `AutoProceed` for everything by default; the gate
//! can be customised to require operator approval, observe fleet conflicts,
//! or abort the sortie.

use hivemind_protocol::{Sortie, SortieStep};

use crate::domain::plan::PlanId;

#[derive(Debug, Clone)]
pub enum Gate {
    /// Default: legion can start the next step immediately.
    AutoProceed,
    /// The slicer or operator marked this step as needing explicit approval.
    OperatorRequired { reason: String },
    /// Layer 2 fleet monitor flagged a conflict; legion must hold until
    /// cleared.
    FleetConflict { with: String },
    /// Unrecoverable: abort the whole sortie.
    AbortSortie { reason: String },
}

/// Context handed to a `GateEvaluator`. v1 carries a minimal subset; v2 will
/// extend this with fleet snapshot and weather.
pub struct GateContext<'a> {
    pub plan_id: PlanId,
    pub sortie: &'a Sortie,
    pub step: &'a SortieStep,
}

/// Decision an operator made on a gated step. Used by the operator-required
/// path in the supervisor.
#[derive(Debug, Clone)]
pub enum OperatorDecision {
    Proceed,
    Hold,
    AbortSortie { reason: String },
}

/// Trait so tests can swap in a gate that returns whatever they need.
pub trait GateEvaluator: Send + Sync {
    fn evaluate(&self, ctx: GateContext<'_>) -> Gate;
}

/// Default v1 evaluator: always returns `AutoProceed`.
#[derive(Debug, Default, Clone, Copy)]
pub struct AutoProceedEvaluator;

impl GateEvaluator for AutoProceedEvaluator {
    fn evaluate(&self, _ctx: GateContext<'_>) -> Gate {
        Gate::AutoProceed
    }
}
