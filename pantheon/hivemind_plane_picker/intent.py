"""Pure intent-document builder.

This module has NO Blender dependency. It accepts plain Python data
(dataclasses) and produces a JSON-serializable dict matching the Hivemind v1.0
intent file schema. It is unit-tested in isolation by tests/test_intent.py.

The contract with oracle is documented in pantheon/README.md → "Intent file
format". Do not change the schema without bumping INTENT_SCHEMA_VERSION and
updating that doc.
"""

from __future__ import annotations

import json
from dataclasses import dataclass
from pathlib import Path
from typing import Any

INTENT_SCHEMA_VERSION = "1.0"

Vec3 = tuple[float, float, float]


@dataclass(frozen=True)
class Face:
    """One triangle in world coordinates.

    `vertices` is exactly three points; `normal` is a unit vector pointing
    out of the triangle (toward the side the drone should approach from).
    """

    vertices: tuple[Vec3, Vec3, Vec3]
    normal: Vec3


@dataclass(frozen=True)
class Region:
    """A named paint region — a set of triangles plus computed surface area."""

    id: str
    name: str
    faces: tuple[Face, ...]
    area_m2: float


@dataclass(frozen=True)
class Scan:
    """Source scan metadata.

    `georeferenced` tells oracle whether the vertex coordinates are already
    in real-world units (true) or in arbitrary mesh space (false, requiring
    a GCP alignment step on import).
    """

    id: str
    source_file: str
    georeferenced: bool


def build_intent(
    scan: Scan,
    regions: tuple[Region, ...],
    constraints: dict[str, Any] | None = None,
) -> dict[str, Any]:
    """Return a JSON-serializable dict matching the v1.0 intent schema."""
    return {
        "version": INTENT_SCHEMA_VERSION,
        "scan": {
            "id": scan.id,
            "source_file": scan.source_file,
            "georeferenced": scan.georeferenced,
        },
        "regions": [
            {
                "id": region.id,
                "name": region.name,
                "faces": [
                    {
                        "vertices": [list(v) for v in face.vertices],
                        "normal": list(face.normal),
                    }
                    for face in region.faces
                ],
                "area_m2": region.area_m2,
            }
            for region in regions
        ],
        "constraints": dict(constraints) if constraints else {},
    }


def write_intent(
    path: str | Path,
    scan: Scan,
    regions: tuple[Region, ...],
    constraints: dict[str, Any] | None = None,
) -> None:
    """Serialize an intent document to a UTF-8 JSON file."""
    document = build_intent(scan, regions, constraints)
    Path(path).write_text(json.dumps(document, indent=2), encoding="utf-8")
