//! Intent types — exactly the shape pantheon's intent.json deserialises into.
//!
//! See [pantheon/README.md → Intent file format] for the canonical schema.
//!
//! [pantheon/README.md → Intent file format]: ../../../pantheon/README.md#intent-file-format-v10

use serde::{Deserialize, Serialize};

/// The top-level intent file. One of these is produced by pantheon's Blender
/// add-on per scan and uploaded to oracle as JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Intent {
    pub version: String,
    pub scan: ScanRef,
    pub regions: Vec<MeshRegion>,
    #[serde(default)]
    pub constraints: OperatorConstraints,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanRef {
    pub id: String,
    #[serde(default)]
    pub source_file: Option<String>,
    pub georeferenced: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshRegion {
    pub id: String,
    pub name: String,
    pub faces: Vec<Face>,
    pub area_m2: f64,
    #[serde(default)]
    pub paint_spec: Option<PaintSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Face {
    /// Three triangle vertices in world coordinates (local ENU around the
    /// truck origin if `georeferenced = true`, mesh-space otherwise).
    pub vertices: [[f64; 3]; 3],
    /// Outward normal — tells the slicer which side of the surface to spray
    /// from.
    pub normal: [f64; 3],
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PaintSpec {
    #[serde(default)]
    pub paint_type: Option<String>,
    #[serde(default)]
    pub thickness_um: Option<f32>,
    #[serde(default)]
    pub coats: Option<u32>,
}

/// Operator-set constraints that the slicer reads but does not interpret
/// directly — for v1 this is opaque metadata that may carry hints like
/// "prefer morning passes" or "no-fly zones." Stored as JSON in the database.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct OperatorConstraints {
    /// Preferred time window in ISO 8601, optional.
    pub time_window: Option<String>,
    /// Maximum drones permitted simultaneously, optional.
    pub max_concurrent_drones: Option<u32>,
    /// Free-form notes carried through to the audit log.
    pub notes: Option<String>,
}
