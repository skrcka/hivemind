//! Unforgeable command authority — the type-system enforcement of "every
//! command to a drone must come from an Approved plan or the safety override
//! path."
//!
//! `CommandAuthority` is a small marker struct with a private constructor.
//! Only [`super::Link::authority_for_plan`] (called by the Apply Supervisor
//! when it starts) and the safety-override path can mint one. Code review
//! enforces the discipline; module privacy enforces the type-level invariant
//! that you can't construct a `CommandAuthority` from outside this module.

use crate::domain::plan::PlanId;

/// Witness that a command is authorised by an Approved plan.
#[derive(Debug, Clone, Copy)]
pub struct CommandAuthority {
    plan_id: PlanId,
}

impl CommandAuthority {
    /// Create an authority for an Approved plan. Only the Apply Supervisor
    /// `start()` should call this.
    pub(super) fn for_approved_plan(plan_id: PlanId) -> Self {
        Self { plan_id }
    }

    pub fn plan_id(&self) -> PlanId {
        self.plan_id
    }
}

/// Coarser authority for safety-override commands which don't belong to a
/// specific plan (Layer 2 holds, fleet-wide aborts, RTK distribution).
#[derive(Debug, Clone, Copy)]
pub enum AuthorityKind {
    /// Tied to a specific plan; pass through the supervisor's authority.
    Plan(CommandAuthority),
    /// Safety/abort/RTK path. Mintable by [`super::Link::safety_override_authority`].
    SafetyOverride,
}

#[derive(Debug, Clone, Copy)]
pub enum HoldReason {
    Conflict,
    Operator,
    Weather,
}

impl HoldReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Conflict => "fleet_conflict",
            Self::Operator => "operator_pause",
            Self::Weather => "weather_hold",
        }
    }
}
