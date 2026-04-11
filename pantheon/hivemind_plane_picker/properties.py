"""PropertyGroups for storing Hivemind region data on a mesh.

Region data lives on `bpy.types.Mesh` (not `Object`) so that duplicating an
object that shares the same mesh keeps the regions consistent across instances.

Storage shape:

    mesh.hivemind_regions[i].name           : str
    mesh.hivemind_regions[i].face_indices[] : sub-collection of HivemindFaceIndex
    mesh.hivemind_active_region_index       : int  (selection in the UIList)
    mesh.hivemind_scan_id                   : str  (intent.scan.id)
    mesh.hivemind_georeferenced             : bool (intent.scan.georeferenced)
"""

from __future__ import annotations

import bpy
from bpy.props import (
    BoolProperty,
    CollectionProperty,
    IntProperty,
    StringProperty,
)
from bpy.types import Mesh, PropertyGroup


class HivemindFaceIndex(PropertyGroup):
    """One face index belonging to a region.

    Wrapped in a PropertyGroup because Blender's CollectionProperty cannot
    hold raw integers — only PropertyGroup instances.
    """

    index: IntProperty(name="Index", default=0, min=0)


class HivemindRegion(PropertyGroup):
    """A named group of mesh faces that should be painted."""

    name: StringProperty(
        name="Name",
        default="region",
        description="Region identifier — written as the region id in the exported intent file",
    )
    face_indices: CollectionProperty(type=HivemindFaceIndex)


_classes = (HivemindFaceIndex, HivemindRegion)


def register() -> None:
    for cls in _classes:
        bpy.utils.register_class(cls)

    Mesh.hivemind_regions = CollectionProperty(type=HivemindRegion)
    Mesh.hivemind_active_region_index = IntProperty(default=0, min=0)
    Mesh.hivemind_scan_id = StringProperty(
        name="Scan ID",
        default="",
        description="Stable identifier for this scan, written into the intent file",
    )
    Mesh.hivemind_georeferenced = BoolProperty(
        name="Georeferenced",
        default=False,
        description=(
            "Set to true if vertex coordinates are already in real-world units. "
            "If false, oracle will require a GCP alignment step on import."
        ),
    )


def unregister() -> None:
    del Mesh.hivemind_georeferenced
    del Mesh.hivemind_scan_id
    del Mesh.hivemind_active_region_index
    del Mesh.hivemind_regions

    for cls in reversed(_classes):
        bpy.utils.unregister_class(cls)
