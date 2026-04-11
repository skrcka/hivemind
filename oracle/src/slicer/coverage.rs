//! Coverage stage: turn each region into a list of straight-line spray
//! passes parallel to one principal axis of the region's bounding rectangle.
//!
//! Each input region must be near-planar — every face's normal within
//! `cfg.planarity_tol_deg` of the region's average normal — to be planned as
//! a single rectangle. **Non-planar regions are auto-subdivided** by
//! clustering face normals into groups that each satisfy the planarity
//! tolerance, and each cluster becomes its own sub-region for the rest of
//! the slicer pipeline.

use glam::Vec3;

use crate::config::SlicerConfig;
use crate::domain::{
    intent::{Face, Intent, MeshRegion},
    plan::{CoveragePlan, PlanError, PlanErrorCode, PlanWarning, PlanWarningCode, PlanWarningSeverity},
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
    pub warnings: Vec<PlanWarning>,
    pub errors: Vec<PlanError>,
}

pub fn generate_passes(intent: &Intent, cfg: &SlicerConfig) -> CoverageResult {
    let mut passes = Vec::new();
    let mut warnings = Vec::new();
    let mut errors = Vec::new();
    let mut total_area_m2 = 0.0_f64;

    for region in &intent.regions {
        // 1. Try the region whole. If it's planar, plan it as one rectangle.
        if is_planar(region, cfg) {
            match passes_for_region(region, cfg) {
                Ok(region_passes) => {
                    total_area_m2 += region.area_m2;
                    passes.extend(region_passes);
                }
                Err(e) => errors.push(e),
            }
            continue;
        }

        // 2. Region is non-planar — cluster faces by normal direction.
        let cluster_groups = cluster_faces_by_normal(&region.faces, cfg.planarity_tol_deg);

        if cluster_groups.is_empty() {
            errors.push(PlanError {
                code: PlanErrorCode::NonPlanarRegion,
                message: "non-planar region; clustering produced no sub-regions".into(),
                region_id: Some(region.id.clone()),
            });
            continue;
        }

        warnings.push(PlanWarning {
            severity: PlanWarningSeverity::Info,
            code: PlanWarningCode::RegionSubdivided,
            message: format!(
                "region '{}' is non-planar; auto-split into {} planar sub-regions",
                region.id,
                cluster_groups.len()
            ),
        });

        for (i, face_indices) in cluster_groups.iter().enumerate() {
            let sub_faces: Vec<Face> =
                face_indices.iter().map(|&idx| region.faces[idx].clone()).collect();
            let sub_area_m2 = sub_faces.iter().map(triangle_area).sum::<f64>();
            let sub_region = MeshRegion {
                id: format!("{}__sub{i:02}", region.id),
                name: format!("{} (sub {})", region.name, i + 1),
                faces: sub_faces,
                area_m2: sub_area_m2,
                paint_spec: region.paint_spec.clone(),
            };

            match passes_for_region(&sub_region, cfg) {
                Ok(sub_passes) => {
                    total_area_m2 += sub_area_m2;
                    passes.extend(sub_passes);
                }
                Err(e) => errors.push(e),
            }
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
        warnings,
        errors,
    }
}

/// Cheap planarity check: does every face's normal agree with the region's
/// average normal within `cfg.planarity_tol_deg`?
fn is_planar(region: &MeshRegion, cfg: &SlicerConfig) -> bool {
    if region.faces.is_empty() {
        return false;
    }
    let normals: Vec<Vec3> = region.faces.iter().map(face_normal_vec).collect();
    let avg = geometry::average_normal(&normals);
    if avg.length_squared() < 0.5 {
        return false;
    }
    let cos_tol = cfg.planarity_tol_deg.to_radians().cos();
    normals
        .iter()
        .all(|n| n.normalize_or_zero().dot(avg) >= cos_tol)
}

/// Greedy clustering of face normals. Each face joins the existing cluster
/// whose running-average normal is within `tol_deg`; if none qualifies, the
/// face spawns a new cluster. Returns one `Vec<usize>` per cluster
/// containing indices into the original `faces` slice.
///
/// Deterministic given the same input order. O(n·k) where k is the number
/// of clusters discovered.
fn cluster_faces_by_normal(faces: &[Face], tol_deg: f32) -> Vec<Vec<usize>> {
    let cos_tol = tol_deg.to_radians().cos();
    let mut clusters: Vec<Cluster> = Vec::new();

    for (idx, face) in faces.iter().enumerate() {
        let n = face_normal_vec(face);
        if n.length_squared() < 0.5 {
            continue; // skip degenerate normals
        }

        // Pick the existing cluster with the highest dot-product agreement
        // (most aligned), provided it clears the tolerance.
        let mut best: Option<(usize, f32)> = None;
        for (ci, cluster) in clusters.iter().enumerate() {
            let dot = n.dot(cluster.centroid_normal);
            if dot >= cos_tol && best.map_or(true, |(_, prev)| dot > prev) {
                best = Some((ci, dot));
            }
        }

        if let Some((ci, _)) = best {
            clusters[ci].add(idx, n);
        } else {
            clusters.push(Cluster::new(idx, n));
        }
    }

    clusters.into_iter().map(|c| c.face_indices).collect()
}

#[derive(Debug)]
struct Cluster {
    face_indices: Vec<usize>,
    /// Running average of every member's normal, normalised.
    centroid_normal: Vec3,
}

impl Cluster {
    fn new(face_idx: usize, normal: Vec3) -> Self {
        Self {
            face_indices: vec![face_idx],
            centroid_normal: normal,
        }
    }

    fn add(&mut self, face_idx: usize, normal: Vec3) {
        // Update centroid as a running average (weighted by member count).
        #[allow(clippy::cast_precision_loss)]
        let n = self.face_indices.len() as f32;
        let updated = (self.centroid_normal * n + normal) / (n + 1.0);
        self.centroid_normal = updated.normalize_or_zero();
        self.face_indices.push(face_idx);
    }
}

fn face_normal_vec(face: &Face) -> Vec3 {
    #[allow(clippy::cast_possible_truncation)]
    Vec3::new(
        face.normal[0] as f32,
        face.normal[1] as f32,
        face.normal[2] as f32,
    )
}

/// Geometric area of one triangular face — used to recompute area for
/// auto-subdivided sub-regions, since the intent's `area_m2` is for the
/// whole original region.
fn triangle_area(face: &Face) -> f64 {
    let v: [glam::DVec3; 3] = [
        glam::DVec3::new(face.vertices[0][0], face.vertices[0][1], face.vertices[0][2]),
        glam::DVec3::new(face.vertices[1][0], face.vertices[1][1], face.vertices[1][2]),
        glam::DVec3::new(face.vertices[2][0], face.vertices[2][1], face.vertices[2][2]),
    ];
    0.5 * (v[1] - v[0]).cross(v[2] - v[0]).length()
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

    let mut passes = Vec::new();

    if v_extent < step_v {
        // Region is narrower than a single spray pass — emit one centred pass
        // along u so the drone still covers it at all.
        let v_centre = (v_min + v_max) / 2.0;
        let start = geometry::unproject(u_min, v_centre, 0.0, centre, u_axis, v_axis, avg_normal);
        let end = geometry::unproject(u_max, v_centre, 0.0, centre, u_axis, v_axis, avg_normal);
        passes.push(SprayPass {
            region_id: region.id.clone(),
            start_enu: start,
            end_enu: end,
            normal: avg_normal,
        });
    } else {
        // Centre the lattice on (v_min + step_v/2) so passes are inside the
        // bbox. Boustrophedon: alternate the direction of every other pass so
        // the drone doesn't have to fly back to u_min between passes.
        let mut v = v_min + step_v / 2.0;
        let mut alternate = false;
        while v <= v_max - step_v / 2.0 + f32::EPSILON {
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
    }

    Ok(passes)
}
