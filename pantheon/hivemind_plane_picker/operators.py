"""Blender operators for the Hivemind Plane Picker add-on.

Each operator follows the same pattern:

  1. `poll()` filters the UI button so it only enables when the active object
     is a mesh (and, where relevant, has at least one region).
  2. `invoke()` opens any dialog or file picker and seeds defaults.
  3. `execute()` does the actual work, reports a success/error message via
     `self.report()`, and returns the appropriate operator status set.

The export operator delegates the schema construction to `intent.py` so that
all the bpy-free, deterministic logic stays in one isolated, testable module.
"""

from __future__ import annotations

from pathlib import Path

import bmesh
import bpy
from bpy.props import StringProperty
from bpy.types import Context, Mesh, Object, Operator
from bpy_extras.io_utils import ExportHelper

from .intent import Face, Region, Scan, write_intent

# ─── Helpers ────────────────────────────────────────────────────────────────


def _active_mesh_object(context: Context) -> Object | None:
    """Return the active object iff it is a mesh, else None."""
    obj = context.active_object
    if obj is None or obj.type != "MESH":
        return None
    return obj


def _selected_face_indices(obj: Object) -> list[int]:
    """Return polygon indices currently selected on `obj`, in any mode."""
    mesh: Mesh = obj.data
    if obj.mode == "EDIT":
        bm = bmesh.from_edit_mesh(mesh)
        bm.faces.ensure_lookup_table()
        return [f.index for f in bm.faces if f.select]
    return [p.index for p in mesh.polygons if p.select]


def _set_selected_face_indices(obj: Object, indices: set[int]) -> None:
    """Replace the current face selection with `indices`."""
    mesh: Mesh = obj.data
    if obj.mode == "EDIT":
        bm = bmesh.from_edit_mesh(mesh)
        bm.faces.ensure_lookup_table()
        for face in bm.faces:
            face.select_set(face.index in indices)
        bmesh.update_edit_mesh(mesh)
    else:
        for poly in mesh.polygons:
            poly.select = poly.index in indices


def _find_region_by_name(mesh: Mesh, name: str) -> int:
    """Return the index of the region with the given name, or -1 if not found."""
    for i, region in enumerate(mesh.hivemind_regions):
        if region.name == name:
            return i
    return -1


def _build_region_payload(obj: Object, region_props) -> Region:
    """Convert a stored region (face-index list) into an `intent.Region`.

    Each marked polygon is fan-triangulated into world-space triangles. For
    each triangle the world-space normal and area are recomputed from the
    transformed vertices, so the result is correct under arbitrary object
    transforms (translation, rotation, non-uniform scale).

    Stale face indices (faces deleted after the region was marked) are
    silently dropped — degenerate triangles too. We don't fail the export on
    a stale index because the user can always re-mark the region.
    """
    mesh: Mesh = obj.data
    matrix = obj.matrix_world
    n_polygons = len(mesh.polygons)

    indices = {fi.index for fi in region_props.face_indices}

    faces: list[Face] = []
    total_area = 0.0

    for poly_index in sorted(indices):
        if poly_index >= n_polygons:
            continue  # stale index — face was deleted after marking

        poly = mesh.polygons[poly_index]
        world_verts = [matrix @ mesh.vertices[v].co for v in poly.vertices]

        # Fan triangulate around the first vertex.
        v0 = world_verts[0]
        for i in range(1, len(world_verts) - 1):
            v1 = world_verts[i]
            v2 = world_verts[i + 1]
            edge1 = v1 - v0
            edge2 = v2 - v0
            cross = edge1.cross(edge2)
            length = cross.length
            if length == 0.0:
                continue  # degenerate triangle

            area = 0.5 * length
            normal = cross / length
            faces.append(
                Face(
                    vertices=(
                        (v0.x, v0.y, v0.z),
                        (v1.x, v1.y, v1.z),
                        (v2.x, v2.y, v2.z),
                    ),
                    normal=(normal.x, normal.y, normal.z),
                )
            )
            total_area += area

    return Region(
        id=region_props.name,
        name=region_props.name,
        faces=tuple(faces),
        area_m2=total_area,
    )


# ─── Operators ──────────────────────────────────────────────────────────────


class HIVEMIND_OT_mark_region(Operator):
    """Mark the current face selection as a paint region."""

    bl_idname = "hivemind.mark_region"
    bl_label = "Mark Selected as Region"
    bl_options = {"REGISTER", "UNDO"}

    region_name: StringProperty(
        name="Region name",
        default="region",
        description="Identifier for the region; must be unique on this mesh",
    )

    @classmethod
    def poll(cls, context: Context) -> bool:
        return _active_mesh_object(context) is not None

    def invoke(self, context: Context, event) -> set[str]:
        obj = _active_mesh_object(context)
        # Suggest a unique default name like region, region.002, region.003 …
        existing = {r.name for r in obj.data.hivemind_regions}
        candidate = "region"
        n = 1
        while candidate in existing:
            n += 1
            candidate = f"region.{n:03d}"
        self.region_name = candidate
        return context.window_manager.invoke_props_dialog(self)

    def execute(self, context: Context) -> set[str]:
        obj = _active_mesh_object(context)
        if obj is None:
            self.report({"ERROR"}, "Active object is not a mesh")
            return {"CANCELLED"}

        name = self.region_name.strip()
        if not name:
            self.report({"ERROR"}, "Region name cannot be empty")
            return {"CANCELLED"}

        if _find_region_by_name(obj.data, name) >= 0:
            self.report({"ERROR"}, f"Region '{name}' already exists")
            return {"CANCELLED"}

        selected = _selected_face_indices(obj)
        if not selected:
            self.report({"ERROR"}, "No faces selected")
            return {"CANCELLED"}

        region = obj.data.hivemind_regions.add()
        region.name = name
        for face_index in selected:
            entry = region.face_indices.add()
            entry.index = face_index

        obj.data.hivemind_active_region_index = len(obj.data.hivemind_regions) - 1

        self.report({"INFO"}, f"Marked {len(selected)} faces as '{name}'")
        return {"FINISHED"}


class HIVEMIND_OT_select_region(Operator):
    """Select all faces belonging to the active region in the list."""

    bl_idname = "hivemind.select_region"
    bl_label = "Select"
    bl_options = {"REGISTER", "UNDO"}

    @classmethod
    def poll(cls, context: Context) -> bool:
        obj = _active_mesh_object(context)
        return obj is not None and len(obj.data.hivemind_regions) > 0

    def execute(self, context: Context) -> set[str]:
        obj = _active_mesh_object(context)
        if obj is None:
            return {"CANCELLED"}

        regions = obj.data.hivemind_regions
        idx = obj.data.hivemind_active_region_index
        if idx < 0 or idx >= len(regions):
            self.report({"ERROR"}, "No active region")
            return {"CANCELLED"}

        region = regions[idx]
        indices = {fi.index for fi in region.face_indices}
        _set_selected_face_indices(obj, indices)

        self.report({"INFO"}, f"Selected {len(indices)} faces from '{region.name}'")
        return {"FINISHED"}


class HIVEMIND_OT_rename_region(Operator):
    """Rename the active region."""

    bl_idname = "hivemind.rename_region"
    bl_label = "Rename"
    bl_options = {"REGISTER", "UNDO"}

    new_name: StringProperty(name="New name", default="")

    @classmethod
    def poll(cls, context: Context) -> bool:
        obj = _active_mesh_object(context)
        return obj is not None and len(obj.data.hivemind_regions) > 0

    def invoke(self, context: Context, event) -> set[str]:
        obj = _active_mesh_object(context)
        idx = obj.data.hivemind_active_region_index
        if 0 <= idx < len(obj.data.hivemind_regions):
            self.new_name = obj.data.hivemind_regions[idx].name
        return context.window_manager.invoke_props_dialog(self)

    def execute(self, context: Context) -> set[str]:
        obj = _active_mesh_object(context)
        if obj is None:
            return {"CANCELLED"}

        regions = obj.data.hivemind_regions
        idx = obj.data.hivemind_active_region_index
        if idx < 0 or idx >= len(regions):
            return {"CANCELLED"}

        new_name = self.new_name.strip()
        if not new_name:
            self.report({"ERROR"}, "Name cannot be empty")
            return {"CANCELLED"}

        for i, region in enumerate(regions):
            if i != idx and region.name == new_name:
                self.report({"ERROR"}, f"Region '{new_name}' already exists")
                return {"CANCELLED"}

        regions[idx].name = new_name
        return {"FINISHED"}


class HIVEMIND_OT_remove_region(Operator):
    """Remove the active region from the list."""

    bl_idname = "hivemind.remove_region"
    bl_label = "Remove"
    bl_options = {"REGISTER", "UNDO"}

    @classmethod
    def poll(cls, context: Context) -> bool:
        obj = _active_mesh_object(context)
        return obj is not None and len(obj.data.hivemind_regions) > 0

    def execute(self, context: Context) -> set[str]:
        obj = _active_mesh_object(context)
        if obj is None:
            return {"CANCELLED"}

        regions = obj.data.hivemind_regions
        idx = obj.data.hivemind_active_region_index
        if idx < 0 or idx >= len(regions):
            return {"CANCELLED"}

        name = regions[idx].name
        regions.remove(idx)

        # Keep the active index in range after removal.
        new_count = len(regions)
        if new_count == 0:
            obj.data.hivemind_active_region_index = 0
        else:
            obj.data.hivemind_active_region_index = min(idx, new_count - 1)

        self.report({"INFO"}, f"Removed region '{name}'")
        return {"FINISHED"}


class HIVEMIND_OT_clear_regions(Operator):
    """Remove every region from the active mesh."""

    bl_idname = "hivemind.clear_regions"
    bl_label = "Clear All Regions"
    bl_options = {"REGISTER", "UNDO"}

    @classmethod
    def poll(cls, context: Context) -> bool:
        obj = _active_mesh_object(context)
        return obj is not None and len(obj.data.hivemind_regions) > 0

    def invoke(self, context: Context, event) -> set[str]:
        return context.window_manager.invoke_confirm(
            self,
            event,
            title="Clear all regions?",
            message="This removes every marked region from the active mesh.",
            icon="WARNING",
        )

    def execute(self, context: Context) -> set[str]:
        obj = _active_mesh_object(context)
        if obj is None:
            return {"CANCELLED"}

        n = len(obj.data.hivemind_regions)
        obj.data.hivemind_regions.clear()
        obj.data.hivemind_active_region_index = 0

        self.report({"INFO"}, f"Cleared {n} regions")
        return {"FINISHED"}


class HIVEMIND_OT_export_intent(Operator, ExportHelper):
    """Export every marked region on the active mesh as a Hivemind intent JSON file."""

    bl_idname = "hivemind.export_intent"
    bl_label = "Export Intent…"

    filename_ext = ".json"
    filter_glob: StringProperty(default="*.json", options={"HIDDEN"})

    @classmethod
    def poll(cls, context: Context) -> bool:
        obj = _active_mesh_object(context)
        return obj is not None and len(obj.data.hivemind_regions) > 0

    def invoke(self, context: Context, event) -> set[str]:
        obj = _active_mesh_object(context)
        scan_id = obj.data.hivemind_scan_id.strip() or obj.data.name
        self.filepath = f"{scan_id}_intent.json"
        return super().invoke(context, event)

    def execute(self, context: Context) -> set[str]:
        obj = _active_mesh_object(context)
        if obj is None:
            return {"CANCELLED"}

        mesh: Mesh = obj.data
        if len(mesh.hivemind_regions) == 0:
            self.report({"ERROR"}, "No regions marked")
            return {"CANCELLED"}

        scan = Scan(
            id=mesh.hivemind_scan_id.strip() or mesh.name,
            source_file=bpy.data.filepath or "",
            georeferenced=bool(mesh.hivemind_georeferenced),
        )

        regions = tuple(_build_region_payload(obj, props) for props in mesh.hivemind_regions)

        try:
            write_intent(self.filepath, scan, regions)
        except OSError as exc:
            self.report({"ERROR"}, f"Failed to write {self.filepath}: {exc}")
            return {"CANCELLED"}

        total_triangles = sum(len(r.faces) for r in regions)
        total_area = sum(r.area_m2 for r in regions)
        self.report(
            {"INFO"},
            f"Exported {len(regions)} regions ({total_triangles} triangles, "
            f"{total_area:.2f} m²) to {Path(self.filepath).name}",
        )
        return {"FINISHED"}


# ─── Registration ───────────────────────────────────────────────────────────


_classes = (
    HIVEMIND_OT_mark_region,
    HIVEMIND_OT_select_region,
    HIVEMIND_OT_rename_region,
    HIVEMIND_OT_remove_region,
    HIVEMIND_OT_clear_regions,
    HIVEMIND_OT_export_intent,
)


def register() -> None:
    for cls in _classes:
        bpy.utils.register_class(cls)


def unregister() -> None:
    for cls in reversed(_classes):
        bpy.utils.unregister_class(cls)
