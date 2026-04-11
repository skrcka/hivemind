"""Panel section in the Hivemind 3D-viewport N-panel.

Contributes a separate `Plan Preview` panel under the same `Hivemind` tab as
`hivemind_plane_picker`. Multiple add-ons can share a tab category — Blender
just stacks panels with the same `bl_category` together.
"""

from __future__ import annotations

import bpy
from bpy.types import Context, Panel


class HIVEMIND_PT_plan_preview(Panel):
    bl_label = "Plan Preview"
    bl_idname = "HIVEMIND_PT_plan_preview"
    bl_space_type = "VIEW_3D"
    bl_region_type = "UI"
    bl_category = "Hivemind"

    def draw(self, context: Context) -> None:
        layout = self.layout
        layout.label(text="Visualize a HivemindPlan", icon="ANIM")
        layout.operator("hivemind.build_plan_preview", icon="IMPORT")


_classes = (HIVEMIND_PT_plan_preview,)


def register() -> None:
    for cls in _classes:
        bpy.utils.register_class(cls)


def unregister() -> None:
    for cls in reversed(_classes):
        bpy.utils.unregister_class(cls)
