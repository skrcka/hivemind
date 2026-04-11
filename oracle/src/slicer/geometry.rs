//! Geometry helpers — ENU ↔ WGS84 conversion, plane bases, projections.

use glam::{DVec3, Vec3};

use crate::config::SlicerConfig;

/// One degree of latitude in metres at the equator (close enough at any
/// latitude for our flat-Earth approximation).
const METRES_PER_DEG_LAT: f64 = 111_320.0;

/// Convert a local ENU offset (east, north, up) in metres to a WGS84 lat/lon
/// using a flat-Earth approximation around the configured truck origin. Good
/// to a few centimetres for points within ~10 km of the origin.
pub fn enu_to_wgs84(enu: DVec3, cfg: &SlicerConfig) -> (f64, f64, f32) {
    let origin_lat_rad = cfg.origin_lat_deg.to_radians();
    let metres_per_deg_lon = METRES_PER_DEG_LAT * origin_lat_rad.cos();
    let lat = cfg.origin_lat_deg + enu.y / METRES_PER_DEG_LAT;
    let lon = cfg.origin_lon_deg + enu.x / metres_per_deg_lon;
    #[allow(clippy::cast_possible_truncation)]
    let alt_m = cfg.origin_alt_m + enu.z as f32;
    (lat, lon, alt_m)
}

/// Build an orthonormal 2D basis on the plane perpendicular to `normal`.
/// Returns `(u_axis, v_axis)` such that `normal`, `u_axis`, `v_axis` form a
/// right-handed triple.
pub fn plane_basis(normal: Vec3) -> (Vec3, Vec3) {
    let n = normal.normalize_or_zero();
    // Pick a reference vector that's not parallel to the normal.
    let ref_vec = if n.y.abs() < 0.9 {
        Vec3::Y
    } else {
        Vec3::X
    };
    let u = n.cross(ref_vec).normalize_or_zero();
    let v = n.cross(u).normalize_or_zero();
    (u, v)
}

/// Average a slice of normals (assumes they all roughly agree).
pub fn average_normal(normals: &[Vec3]) -> Vec3 {
    let sum: Vec3 = normals.iter().copied().sum();
    sum.normalize_or_zero()
}

/// Compute centroid of a set of points.
pub fn centroid(points: &[Vec3]) -> Vec3 {
    if points.is_empty() {
        return Vec3::ZERO;
    }
    #[allow(clippy::cast_precision_loss)]
    let n = points.len() as f32;
    let sum: Vec3 = points.iter().copied().sum();
    sum / n
}

/// Project a 3D point onto a 2D plane parameterised by `(u_axis, v_axis)`
/// originating at `centre`.
pub fn project(point: Vec3, centre: Vec3, u_axis: Vec3, v_axis: Vec3) -> (f32, f32) {
    let rel = point - centre;
    (rel.dot(u_axis), rel.dot(v_axis))
}

/// Reverse of [`project`]: take a 2D `(u, v)` coordinate plus a normal-axis
/// offset and turn it back into a 3D point.
pub fn unproject(
    u: f32,
    v: f32,
    normal_offset: f32,
    centre: Vec3,
    u_axis: Vec3,
    v_axis: Vec3,
    normal: Vec3,
) -> Vec3 {
    centre + u_axis * u + v_axis * v + normal * normal_offset
}
