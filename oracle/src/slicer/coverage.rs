//! Coverage stage: turn each region into a list of straight-line spray
//! passes parallel to one principal axis of the region's bounding rectangle.
//!
//! v1 only handles flat or near-flat regions. A region is "near-flat" if
//! every face's normal is within `cfg.planarity_tol_deg` of the region's
//! average normal.

use glam::Vec3;

use crate::config::SlicerConfig;
use crate::domain::{
    intent::{Intent, MeshRegion},
    plan::{CoveragePlan, PlanError, PlanErrorCode},
};

use super::geometry;

/// One spray pass: a straight line on the surface, traversed at a fixed
/// standoff distance. Both endpoints are in **local ENU** (metres around the
/// truck origin); the step-assembly stage converts to lat/lon.
#[derive(Debug, Clone)]
pub struct SprayPass {
    pub region_id: String,
    pub start_enu: Vec3,
    pub end_enu: Vec3,
    /// Outward normal at the surface (pointing into free space). The drone
    /// flies at `start_enu + normal * standoff`.
    pub normal: Vec3,
}

impl SprayPass {
    /// Length of the pass in metres (used by resources estimation).
    pub fn length_m(&self) -> f32 {
        (self.end_enu - self.start_enu).length()
    }
}

#[derive(Debug)]
pub struct CoverageResult {
    pub coverage_plan: CoveragePlan,
    pub passes: Vec<SprayPass>,
    pub errors: Vec<PlanError>,
}

pub fn generate_passes(intent: &Intent, cfg: &SlicerConfig) -> CoverageResult {
    let mut passes = Vec::new();
    let mut errors = Vec::new();
    let mut total_area_m2 = 0.0_f64;

    for region in &intent.regions {
        match passes_for_region(region, cfg) {
            Ok(region_passes) => {
                total_area_m2 += region.area_m2;
                passes.extend(region_passes);
            }
            Err(e) => errors.push(e),
        }
    }

    let pass_count = u32::try_from(passes.len()).unwrap_or(u32::MAX);

    let coverage_plan = CoveragePlan {
        total_area_m2,
        overlap_pct: cfg.overlap_pct,
        estimated_coats: 1,
        pass_count,
    };

    CoverageResult {
        coverage_plan,
        passes,
        errors,
    }
}

fn passes_for_region(region: &MeshRegion, cfg: &SlicerConfig) -> Result<Vec<SprayPass>, PlanError> {
    if region.faces.is_empty() {
        return Err(PlanError {
            code: PlanErrorCode::NonPlanarRegion,
            message: "region has no faces".into(),
            region_id: Some(region.id.clone()),
        });
    }

    // Collect face normals and vertex positions in ENU.
    #[allow(clippy::cast_possible_truncation)]
    let normals: Vec<Vec3> = region
        .faces
        .iter()
        .map(|f| Vec3::new(f.normal[0] as f32, f.normal[1] as f32, f.normal[2] as f32))
        .collect();
    let avg_normal = geometry::average_normal(&normals);

    if avg_normal.length_squared() < 0.5 {
        return Err(PlanError {
            code: PlanErrorCode::NonPlanarRegion,
            message: "region's face normals do not agree on a direction".into(),
            region_id: Some(region.id.clone()),
        });
    }

    // Planarity check: every face normal must be within tolerance of avg.
    let cos_tol = cfg.planarity_tol_deg.to_radians().cos();
    for n in &normals {
        if n.normalize_or_zero().dot(avg_normal) < cos_tol {
            return Err(PlanError {
                code: PlanErrorCode::NonPlanarRegion,
                message: format!(
                    "face normal deviates more than {:.1}° from region average",
                    cfg.planarity_tol_deg
                ),
                region_id: Some(region.id.clone()),
            });
        }
    }

    #[allow(clippy::cast_possible_truncation)]
    let vertices: Vec<Vec3> = region
        .faces
        .iter()
        .flat_map(|f| {
            f.vertices
                .iter()
                .map(|v| Vec3::new(v[0] as f32, v[1] as f32, v[2] as f32))
        })
        .collect();
    let centre = geometry::centroid(&vertices);

    let (u_axis, v_axis) = geometry::plane_basis(avg_normal);

    // Project all vertices and find the 2D AABB on the plane.
    let mut u_min = f32::INFINITY;
    let mut u_max = f32::NEG_INFINITY;
    let mut v_min = f32::INFINITY;
    let mut v_max = f32::NEG_INFINITY;
    for vert in &vertices {
        let (u, v) = geometry::project(*vert, centre, u_axis, v_axis);
        u_min = u_min.min(u);
        u_max = u_max.max(u);
        v_min = v_min.min(v);
        v_max = v_max.max(v);
    }

    let v_extent = v_max - v_min;
    let u_extent = u_max - u_min;
    if !u_extent.is_finite() || !v_extent.is_finite() || u_extent <= 0.0 || v_extent <= 0.0 {
        return Err(PlanError {
            code: PlanErrorCode::NonPlanarRegion,
            message: "region's projected bounding rectangle is degenerate".into(),
            region_id: Some(region.id.clone()),
        });
    }

    // Generate parallel spray lines along u, spaced by step_v along v.
    let step_v = cfg.spray_width_m * (1.0 - cfg.overlap_pct).max(0.05);
    if step_v <= 0.0 {
        return Err(PlanError {
            code: PlanErrorCode::NonPlanarRegion,
            message: "spray spacing is non-positive (check spray_width / overlap)".into(),
            region_id: Some(region.id.clone()),
        });
    }

    // Centre the lattice on (v_min + step_v/2) so passes are inside the bbox.
    let mut passes = Vec::new();
    let mut v = v_min + step_v / 2.0;
    let mut alternate = false;
    while v <= v_max - step_v / 2.0 + f32::EPSILON {
        // Boustrophedon: alternate the direction of every other pass so the
        // drone doesn't have to fly back to u_min between passes.
        let (u_a, u_b) = if alternate {
            (u_max, u_min)
        } else {
            (u_min, u_max)
        };
        let start = geometry::unproject(u_a, v, 0.0, centre, u_axis, v_axis, avg_normal);
        let end = geometry::unproject(u_b, v, 0.0, centre, u_axis, v_axis, avg_normal);
        passes.push(SprayPass {
            region_id: region.id.clone(),
            start_enu: start,
            end_enu: end,
            normal: avg_normal,
        });
        v += step_v;
        alternate = !alternate;
    }

    Ok(passes)
}
