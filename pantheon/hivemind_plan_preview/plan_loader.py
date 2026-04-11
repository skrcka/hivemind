"""Pure parser for Hivemind plan + intent JSON files.

Turns the slicer's `HivemindPlan` JSON output and pantheon's `intent.json`
into Python dataclasses suitable for the scene builder. Has NO Blender
dependency — importable in stock Python and unit-tested that way.

The dataclasses here are the *internal* shape used by the visualization
tool; they are deliberately tolerant of missing optional fields so the tool
can render plans even before the full HivemindPlan struct in oracle is
finalised. The canonical wire types live in `hivemind-protocol` (Rust);
this module is the JSON-side projection used only by this Blender add-on.
"""

from __future__ import annotations

import json
from dataclasses import dataclass
from pathlib import Path

# ─── Intent ─────────────────────────────────────────────────────────


@dataclass(frozen=True)
class Vertex:
    x: float
    y: float
    z: float


@dataclass(frozen=True)
class Triangle:
    """One triangle in the intent's coordinate frame.

    `vertices` is exactly three points; `normal` points outward (the side
    the drone should approach from).
    """

    vertices: tuple[Vertex, Vertex, Vertex]
    normal: tuple[float, float, float]


@dataclass(frozen=True)
class IntentRegion:
    """A named paint region — a set of triangles plus pre-computed area."""

    id: str
    name: str
    triangles: tuple[Triangle, ...]
    area_m2: float


@dataclass(frozen=True)
class IntentScan:
    """Source scan metadata mirroring pantheon's intent file."""

    id: str
    source_file: str
    georeferenced: bool


@dataclass(frozen=True)
class Intent:
    version: str
    scan: IntentScan
    regions: tuple[IntentRegion, ...]


# ─── Plan ───────────────────────────────────────────────────────────


@dataclass(frozen=True)
class Waypoint:
    """A GPS waypoint as it appears in `hivemind-protocol::Waypoint`."""

    lat: float
    lon: float
    alt_m: float
    yaw_deg: float | None = None


@dataclass(frozen=True)
class SortieStep:
    """One step within a sortie. Mirrors `hivemind-protocol::SortieStep`."""

    index: int
    step_type: str
    waypoint: Waypoint
    path: tuple[Waypoint, ...] | None
    speed_m_s: float
    expected_duration_s: float
    spray: bool = False


@dataclass(frozen=True)
class Sortie:
    """A per-drone sortie. Mirrors `hivemind-protocol::Sortie`, plus a
    `start_time_s` field that the visualization needs but the wire type
    does not (the wire type is gated step-by-step at runtime, the
    visualization needs absolute timestamps for keyframes)."""

    sortie_id: str
    drone_id: str
    steps: tuple[SortieStep, ...]
    expected_duration_s: float
    start_time_s: float = 0.0  # absolute offset from plan start


@dataclass(frozen=True)
class Plan:
    """A loaded `HivemindPlan`."""

    id: str
    intent: Intent | None  # may be None if loaded from a separate file
    sorties: tuple[Sortie, ...]
    total_duration_s: float


# ─── Loaders ────────────────────────────────────────────────────────


def load_intent(path: str | Path) -> Intent:
    """Read a v1.0 intent.json file from disk."""
    data = json.loads(Path(path).read_text(encoding="utf-8"))
    return parse_intent(data)


def load_plan(path: str | Path) -> Plan:
    """Read a HivemindPlan JSON file (oracle's slicer output) from disk."""
    data = json.loads(Path(path).read_text(encoding="utf-8"))
    return parse_plan(data)


def parse_intent(data: dict) -> Intent:
    """Parse a JSON-deserialized intent dict into an `Intent` dataclass."""
    scan_data = data.get("scan", {})
    scan = IntentScan(
        id=scan_data.get("id", ""),
        source_file=scan_data.get("source_file", ""),
        georeferenced=bool(scan_data.get("georeferenced", False)),
    )
    regions = tuple(_parse_region(r) for r in data.get("regions", []))
    return Intent(
        version=data.get("version", "1.0"),
        scan=scan,
        regions=regions,
    )


def parse_plan(data: dict) -> Plan:
    """Parse a JSON-deserialized plan dict into a `Plan` dataclass.

    Sortie start times are taken from the JSON if present; otherwise sorties
    are scheduled sequentially per drone (each of a drone's sorties starts
    when the previous one finishes). This handles the v1 single-drone case
    cleanly and produces a sensible-if-pessimistic timeline for multi-drone
    plans that omit the schedule.
    """
    intent = parse_intent(data["intent"]) if "intent" in data else None
    sorties_raw = data.get("sorties", [])

    drone_clocks: dict[str, float] = {}
    sorties: list[Sortie] = []
    for s_data in sorties_raw:
        drone_id = s_data["drone_id"]
        steps = tuple(_parse_step(s) for s in s_data.get("steps", []))
        duration = float(
            s_data.get("expected_duration_s")
            or sum(st.expected_duration_s for st in steps)
        )
        start = float(s_data.get("start_time_s", drone_clocks.get(drone_id, 0.0)))
        sorties.append(
            Sortie(
                sortie_id=s_data["sortie_id"],
                drone_id=drone_id,
                steps=steps,
                expected_duration_s=duration,
                start_time_s=start,
            )
        )
        drone_clocks[drone_id] = start + duration

    schedule_total = data.get("schedule", {}).get("total_duration_s")
    total_duration = float(
        schedule_total
        if schedule_total is not None
        else max((s.start_time_s + s.expected_duration_s for s in sorties), default=0.0)
    )

    return Plan(
        id=data.get("id", ""),
        intent=intent,
        sorties=tuple(sorties),
        total_duration_s=total_duration,
    )


# ─── Internal helpers ───────────────────────────────────────────────


def _parse_region(data: dict) -> IntentRegion:
    triangles = tuple(_parse_triangle(f) for f in data.get("faces", []))
    return IntentRegion(
        id=data.get("id", ""),
        name=data.get("name", data.get("id", "region")),
        triangles=triangles,
        area_m2=float(data.get("area_m2", 0.0)),
    )


def _parse_triangle(data: dict) -> Triangle:
    verts_raw = data["vertices"]
    if len(verts_raw) != 3:
        raise ValueError(f"Expected 3 vertices per triangle, got {len(verts_raw)}")
    verts = tuple(Vertex(float(v[0]), float(v[1]), float(v[2])) for v in verts_raw)
    normal_raw = data["normal"]
    normal = (float(normal_raw[0]), float(normal_raw[1]), float(normal_raw[2]))
    return Triangle(vertices=verts, normal=normal)


def _parse_step(data: dict) -> SortieStep:
    path_raw = data.get("path")
    path = tuple(_parse_waypoint(w) for w in path_raw) if path_raw else None
    return SortieStep(
        index=int(data["index"]),
        step_type=str(data.get("step_type", "Transit")),
        waypoint=_parse_waypoint(data["waypoint"]),
        path=path,
        speed_m_s=float(data.get("speed_m_s", 0.0)),
        expected_duration_s=float(data.get("expected_duration_s", 0.0)),
        spray=bool(data.get("spray", False)),
    )


def _parse_waypoint(data: dict) -> Waypoint:
    return Waypoint(
        lat=float(data["lat"]),
        lon=float(data["lon"]),
        alt_m=float(data["alt_m"]),
        yaw_deg=data.get("yaw_deg"),
    )
