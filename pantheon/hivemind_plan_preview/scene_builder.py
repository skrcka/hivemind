"""Build a Blender scene from a parsed HivemindPlan + intent.

Imports each intent region's triangles as a mesh object (the walls), creates
one marker per drone, and adds location keyframes that move each marker
through its sortie's waypoints over the plan's wall-clock duration.

This module is the only place in the package where bpy is loaded
unconditionally — keep it isolated so the pure modules (`coords`,
`plan_loader`, `timeline`) stay importable in stock Python for unit tests.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from pathlib import Path

import bpy

from .coords import GpsOrigin, gps_to_enu
from .plan_loader import (
    Intent,
    Plan,
    Sortie,
    Waypoint,
    load_intent,
    load_plan,
)
from .timeline import Pose3, sortie_position_at

# Per-drone palette colours, cycled if there are more drones than entries.
DRONE_COLOURS: list[tuple[float, float, float, float]] = [
    (1.0, 0.2, 0.2, 1.0),  # red
    (0.2, 0.6, 1.0, 1.0),  # blue
    (0.3, 1.0, 0.3, 1.0),  # green
    (1.0, 1.0, 0.2, 1.0),  # yellow
    (1.0, 0.5, 0.0, 1.0),  # orange
    (0.7, 0.3, 1.0, 1.0),  # purple
    (0.0, 1.0, 0.9, 1.0),  # cyan
    (1.0, 0.4, 0.7, 1.0),  # pink
]


@dataclass
class BuildOptions:
    """Knobs for the scene builder."""

    fps: int = 24
    drone_marker_radius: float = 0.5
    origin: GpsOrigin | None = None
    save_path: Path | None = None
    clear_scene: bool = True
    walls_collection_name: str = "walls"
    drones_collection_name: str = "drones"
    sample_every_n_frames: int = 1


@dataclass
class BuildResult:
    """Returned from `build_scene` — useful for the in-Blender operator's
    success report and for tests."""

    walls: int = 0
    drones: int = 0
    keyframes: int = 0
    origin: GpsOrigin | None = None
    total_duration_s: float = 0.0
    warnings: list[str] = field(default_factory=list)


def build_scene(
    plan_path: Path,
    intent_path: Path | None,
    options: BuildOptions,
) -> BuildResult:
    """Top-level entry point: load files, populate the scene, optionally save.

    Returns a `BuildResult` summary; raises `ValueError` for unrecoverable
    input problems (no intent, no waypoints, etc.).
    """
    plan = load_plan(plan_path)
    intent = plan.intent
    if intent is None and intent_path is not None:
        intent = load_intent(intent_path)
    if intent is None:
        raise ValueError(
            "Plan does not embed an intent and no separate --intent file was given."
        )

    origin = options.origin or _pick_origin(plan)
    if origin is None:
        raise ValueError("Cannot pick a GPS origin: plan has no waypoints.")

    if options.clear_scene:
        _clear_scene()

    _setup_scene(plan, options)
    walls_coll = _ensure_collection(options.walls_collection_name)
    drones_coll = _ensure_collection(options.drones_collection_name)

    n_walls = _import_walls(intent, walls_coll)
    n_drones, n_keyframes, warnings = _spawn_drone_markers(
        plan, origin, options, drones_coll
    )

    if options.save_path is not None:
        bpy.ops.wm.save_as_mainfile(filepath=str(options.save_path))

    return BuildResult(
        walls=n_walls,
        drones=n_drones,
        keyframes=n_keyframes,
        origin=origin,
        total_duration_s=plan.total_duration_s,
        warnings=warnings,
    )


# ─── Origin selection ───────────────────────────────────────────────


def _pick_origin(plan: Plan) -> GpsOrigin | None:
    """Default origin: first waypoint of the first sortie that has any."""
    for sortie in plan.sorties:
        for step in sortie.steps:
            wp = step.waypoint
            return GpsOrigin(lat=wp.lat, lon=wp.lon, alt=wp.alt_m)
    return None


# ─── Scene plumbing ─────────────────────────────────────────────────


def _clear_scene() -> None:
    """Delete every object in the active scene."""
    if bpy.context.mode != "OBJECT":
        bpy.ops.object.mode_set(mode="OBJECT")
    bpy.ops.object.select_all(action="SELECT")
    bpy.ops.object.delete(use_global=False)
    # Also drop orphaned meshes / materials so re-runs don't pile up data
    for mesh in list(bpy.data.meshes):
        if mesh.users == 0:
            bpy.data.meshes.remove(mesh)
    for mat in list(bpy.data.materials):
        if mat.users == 0:
            bpy.data.materials.remove(mat)


def _setup_scene(plan: Plan, options: BuildOptions) -> None:
    scene = bpy.context.scene
    scene.frame_start = 1
    scene.frame_end = max(2, int(plan.total_duration_s * options.fps))
    scene.render.fps = options.fps
    scene.unit_settings.system = "METRIC"
    scene.unit_settings.scale_length = 1.0


def _ensure_collection(name: str) -> bpy.types.Collection:
    """Return (creating if needed) a child collection of the active scene."""
    coll = bpy.data.collections.get(name)
    if coll is None:
        coll = bpy.data.collections.new(name)
        bpy.context.scene.collection.children.link(coll)
    return coll


def _link_to_collection(obj: bpy.types.Object, coll: bpy.types.Collection) -> None:
    """Move `obj` into `coll`, removing it from any other parent collection."""
    for existing in list(obj.users_collection):
        existing.objects.unlink(obj)
    coll.objects.link(obj)


# ─── Walls ──────────────────────────────────────────────────────────


def _import_walls(intent: Intent, coll: bpy.types.Collection) -> int:
    """Build one mesh object per region containing all its triangles."""
    n_walls = 0
    for region in intent.regions:
        mesh = bpy.data.meshes.new(name=f"wall_{region.id}")
        verts: list[tuple[float, float, float]] = []
        faces: list[tuple[int, int, int]] = []
        for tri in region.triangles:
            base = len(verts)
            for v in tri.vertices:
                verts.append((v.x, v.y, v.z))
            faces.append((base, base + 1, base + 2))
        mesh.from_pydata(verts, [], faces)
        mesh.update()

        obj = bpy.data.objects.new(name=f"wall_{region.id}", object_data=mesh)
        _link_to_collection(obj, coll)
        n_walls += 1
    return n_walls


# ─── Drones ─────────────────────────────────────────────────────────


def _spawn_drone_markers(
    plan: Plan,
    origin: GpsOrigin,
    options: BuildOptions,
    coll: bpy.types.Collection,
) -> tuple[int, int, list[str]]:
    """Create one marker per drone (not per sortie) and animate it through
    every one of that drone's sorties in order."""
    warnings: list[str] = []

    def to_pose(wp: Waypoint) -> Pose3:
        enu = gps_to_enu(wp.lat, wp.lon, wp.alt_m, origin)
        return Pose3(x=enu.east, y=enu.north, z=enu.up)

    sorties_by_drone: dict[str, list[Sortie]] = {}
    for sortie in plan.sorties:
        sorties_by_drone.setdefault(sortie.drone_id, []).append(sortie)
    for drone_sorties in sorties_by_drone.values():
        drone_sorties.sort(key=lambda s: s.start_time_s)

    total_keyframes = 0
    for drone_index, (drone_id, drone_sorties) in enumerate(sorties_by_drone.items()):
        if not drone_sorties:
            continue
        marker = _make_drone_marker(drone_id, options, coll)
        marker.color = DRONE_COLOURS[drone_index % len(DRONE_COLOURS)]

        n = _animate_drone(
            marker,
            drone_sorties,
            plan.total_duration_s,
            options.fps,
            options.sample_every_n_frames,
            to_pose,
        )
        total_keyframes += n
        if n == 0:
            warnings.append(f"drone {drone_id} produced no keyframes")

    return len(sorties_by_drone), total_keyframes, warnings


def _make_drone_marker(
    drone_id: str,
    options: BuildOptions,
    coll: bpy.types.Collection,
) -> bpy.types.Object:
    """Create a small UV sphere named after the drone and link it to `coll`."""
    bpy.ops.mesh.primitive_uv_sphere_add(
        radius=options.drone_marker_radius,
        location=(0.0, 0.0, 0.0),
    )
    marker = bpy.context.active_object
    marker.name = f"drone_{drone_id}"
    if marker.data is not None:
        marker.data.name = marker.name
    _link_to_collection(marker, coll)
    return marker


def _animate_drone(
    marker: bpy.types.Object,
    sorties: list[Sortie],
    total_duration_s: float,
    fps: int,
    sample_every_n: int,
    to_pose,
) -> int:
    """Set location keyframes on `marker` for the drone's full timeline."""
    if not sorties:
        return 0

    initial_wp = _first_waypoint(sorties[0])
    total_frames = max(2, int(total_duration_s * fps))

    n_keyframes = 0
    sample_step = max(1, sample_every_n)
    frames = list(range(1, total_frames + 1, sample_step))
    if frames[-1] != total_frames:
        frames.append(total_frames)

    for frame in frames:
        t = (frame - 1) / fps
        pose = _drone_position_at(sorties, t, initial_wp, to_pose)
        marker.location = (pose.x, pose.y, pose.z)
        marker.keyframe_insert(data_path="location", frame=frame)
        n_keyframes += 1

    return n_keyframes


def _drone_position_at(
    sorties: list[Sortie],
    t: float,
    initial_wp: Waypoint,
    to_pose,
) -> Pose3:
    """Find the active sortie at time `t` and interpolate within it."""
    active = None
    for sortie in sorties:
        if sortie.start_time_s <= t:
            active = sortie
        else:
            break
    if active is None:
        return to_pose(initial_wp)
    return sortie_position_at(active, t, initial_wp, to_pose)


def _first_waypoint(sortie: Sortie) -> Waypoint:
    """The very first waypoint of a sortie — used as the parking position
    of the drone before the sortie's first step actually starts."""
    if not sortie.steps:
        return Waypoint(lat=0.0, lon=0.0, alt_m=0.0)
    first_step = sortie.steps[0]
    if first_step.path:
        return first_step.path[0]
    return first_step.waypoint
