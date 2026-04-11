"""UI Panel and UIList for the Hivemind Plane Picker add-on.

The whole add-on lives in a single N-panel under the "Hivemind" tab in the 3D
viewport. The panel layout, top to bottom, is:

  • [Mark Selected as Region] button
  • Region list (UIList of HivemindRegion items)
  • [Select] [Rename] [Remove] row beneath the list
  • Scan metadata: scan id + georeferenced flag
  • [Export Intent…] button
  • [Clear All Regions] button
"""

from __future__ import annotations

import bpy
from bpy.types import Context, Panel, UIList


class HIVEMIND_UL_regions(UIList):
    """List view for marked regions on the active mesh."""

    def draw_item(
        self,
        context: Context,
        layout,
        data,
        item,
        icon: int,
        active_data,
        active_propname: str,
        index: int,
    ) -> None:
        if self.layout_type in {"DEFAULT", "COMPACT"}:
            row = layout.row(align=True)
            row.label(text=item.name, icon="MESH_DATA")
            count = len(item.face_indices)
            row.label(text=f"{count} face{'s' if count != 1 else ''}")
        elif self.layout_type == "GRID":
            layout.alignment = "CENTER"
            layout.label(text="", icon="MESH_DATA")


class HIVEMIND_PT_main(Panel):
    """Main Hivemind panel in the 3D viewport's N-panel."""

    bl_label = "Hivemind"
    bl_idname = "HIVEMIND_PT_main"
    bl_space_type = "VIEW_3D"
    bl_region_type = "UI"
    bl_category = "Hivemind"

    def draw(self, context: Context) -> None:
        layout = self.layout
        obj = context.active_object

        if obj is None or obj.type != "MESH":
            box = layout.box()
            box.label(text="Select a mesh object", icon="INFO")
            return

        mesh = obj.data

        # Mark
        layout.operator("hivemind.mark_region", icon="ADD")

        # Region list
        layout.template_list(
            "HIVEMIND_UL_regions",
            "",
            mesh,
            "hivemind_regions",
            mesh,
            "hivemind_active_region_index",
            rows=4,
        )

        row = layout.row(align=True)
        row.operator("hivemind.select_region", icon="RESTRICT_SELECT_OFF")
        row.operator("hivemind.rename_region", text="", icon="GREASEPENCIL")
        row.operator("hivemind.remove_region", text="", icon="X")

        # Scan metadata
        layout.separator()
        col = layout.column(align=True)
        col.label(text="Scan metadata:")
        col.prop(mesh, "hivemind_scan_id")
        col.prop(mesh, "hivemind_georeferenced")

        # Export
        layout.separator()
        layout.operator("hivemind.export_intent", icon="EXPORT")
        layout.operator("hivemind.clear_regions", icon="TRASH")


_classes = (HIVEMIND_UL_regions, HIVEMIND_PT_main)


def register() -> None:
    for cls in _classes:
        bpy.utils.register_class(cls)


def unregister() -> None:
    for cls in reversed(_classes):
        bpy.utils.unregister_class(cls)
