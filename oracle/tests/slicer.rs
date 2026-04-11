//! Slicer unit tests against fixture intents.

use hivemind_protocol::{RadioLossBehaviour, StepType};
use oracle::config::SlicerConfig;
use oracle::domain::{
    fleet::{Drone, DroneState, FleetSnapshot},
    intent::{Face, Intent, MeshRegion, OperatorConstraints, ScanRef},
    plan::PlanErrorCode,
};
use oracle::slicer::{plan, SlicerError};

fn slicer_cfg() -> SlicerConfig {
    SlicerConfig {
        spray_width_m: 0.30,
        overlap_pct: 0.20,
        min_horizontal_separation_m: 3.0,
        battery_safety_margin_pct: 25.0,
        paint_safety_margin_pct: 15.0,
        standoff_m: 0.6,
        origin_lat_deg: 50.0,
        origin_lon_deg: 14.0,
        origin_alt_m: 200.0,
        planarity_tol_deg: 15.0,
        ferry_speed_m_s: 3.0,
        spray_speed_m_s: 0.5,
        takeoff_alt_m: 5.0,
    }
}

fn one_drone() -> FleetSnapshot {
    FleetSnapshot::now(vec![Drone {
        id: "drone-01".into(),
        legion_version: Some("0.1.0".into()),
        capabilities: vec!["spray".into()],
        state: DroneState::default(),
        is_stale: false,
    }])
}

/// A vertical wall facing east at x=10, 4m wide × 3m tall.
fn east_wall_intent() -> Intent {
    let n = [1.0, 0.0, 0.0]; // outward normal pointing east
    // Two triangles forming a 4×3 m rectangle.
    let v00 = [10.0, -2.0, 0.0];
    let v10 = [10.0, 2.0, 0.0];
    let v11 = [10.0, 2.0, 3.0];
    let v01 = [10.0, -2.0, 3.0];
    let faces = vec![
        Face {
            vertices: [v00, v10, v11],
            normal: n,
        },
        Face {
            vertices: [v00, v11, v01],
            normal: n,
        },
    ];
    Intent {
        version: "1.0".into(),
        scan: ScanRef {
            id: "test-east-wall".into(),
            source_file: None,
            georeferenced: true,
        },
        regions: vec![MeshRegion {
            id: "east_wall".into(),
            name: "East wall".into(),
            faces,
            area_m2: 12.0,
            paint_spec: None,
        }],
        constraints: OperatorConstraints::default(),
    }
}

#[test]
fn east_wall_produces_a_valid_plan() {
    let cfg = slicer_cfg();
    let plan = plan(east_wall_intent(), one_drone(), &cfg).expect("slicer should not fail");

    // Plan must be approvable for this clean fixture.
    assert!(
        plan.is_approvable(),
        "expected approvable plan, got errors: {:?}",
        plan.errors
    );

    // One sortie with a Takeoff, at least one Transit, at least one
    // SprayPass, then ReturnToBase, Land.
    assert_eq!(plan.sorties.len(), 1, "v1 packs all work into one sortie");
    let sortie = &plan.sorties[0];
    let step_types: Vec<StepType> = sortie.steps.iter().map(|s| s.step_type).collect();
    assert_eq!(
        step_types.first().copied(),
        Some(StepType::Takeoff),
        "first step must be Takeoff"
    );
    assert_eq!(
        step_types.last().copied(),
        Some(StepType::Land),
        "last step must be Land"
    );
    assert!(
        step_types.contains(&StepType::SprayPass),
        "at least one SprayPass expected"
    );
    assert!(
        step_types.contains(&StepType::ReturnToBase),
        "ReturnToBase expected"
    );

    // Coverage should report the right area and a positive pass count.
    assert!((plan.coverage.total_area_m2 - 12.0).abs() < 0.01);
    assert!(plan.coverage.pass_count > 0);

    // Resources should be positive.
    assert!(plan.resources.paint_ml > 0.0);
    assert!(plan.resources.total_flight_time_s > 0);
}

#[test]
fn radio_loss_policies_are_stamped_per_step_type() {
    let cfg = slicer_cfg();
    let plan = plan(east_wall_intent(), one_drone(), &cfg).expect("slicer ok");
    let sortie = &plan.sorties[0];

    for step in &sortie.steps {
        let policy = &step.radio_loss;
        match step.step_type {
            StepType::Takeoff => {
                assert!(matches!(policy.behaviour, RadioLossBehaviour::HoldThenRtl));
                assert!(policy.hold_then_rtl_after_s.is_some());
            }
            StepType::Transit
            | StepType::SprayPass
            | StepType::ReturnToBase
            | StepType::Land => {
                assert!(matches!(policy.behaviour, RadioLossBehaviour::Continue));
            }
            // Other variants don't show up in v1 sorties.
            _ => {}
        }
    }
}

#[test]
fn empty_intent_fails_with_no_regions() {
    let cfg = slicer_cfg();
    let intent = Intent {
        regions: vec![],
        ..east_wall_intent()
    };
    let err = plan(intent, one_drone(), &cfg).unwrap_err();
    assert!(matches!(err, SlicerError::NoRegions));
}

#[test]
fn non_georeferenced_intent_is_rejected() {
    let cfg = slicer_cfg();
    let mut intent = east_wall_intent();
    intent.scan.georeferenced = false;
    let err = plan(intent, one_drone(), &cfg).unwrap_err();
    assert!(matches!(err, SlicerError::NotGeoreferenced));
}

#[test]
fn empty_fleet_yields_a_plan_with_an_error() {
    let cfg = slicer_cfg();
    let empty_fleet = FleetSnapshot::now(vec![]);
    let plan = plan(east_wall_intent(), empty_fleet, &cfg).expect("slicer still produces a plan");
    assert!(
        !plan.is_approvable(),
        "no drones should produce a non-approvable plan"
    );
    assert!(plan
        .errors
        .iter()
        .any(|e| e.code == PlanErrorCode::NoDronesAvailable));
}

#[test]
fn region_with_inconsistent_normals_is_flagged() {
    let cfg = slicer_cfg();
    let mut intent = east_wall_intent();
    // Replace the second face's normal with one that points the other way.
    intent.regions[0].faces[1].normal = [-1.0, 0.0, 0.0];
    let plan = plan(intent, one_drone(), &cfg).expect("slicer still emits a plan");
    assert!(plan
        .errors
        .iter()
        .any(|e| e.code == PlanErrorCode::NonPlanarRegion));
}
