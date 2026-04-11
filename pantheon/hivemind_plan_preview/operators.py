"""In-Blender operator that builds a plan-preview scene from inside Blender.

The operator opens a properties dialog where the user picks a plan JSON
(required), an intent JSON (optional if the plan embeds it), and a few
build knobs. On execute it delegates to `scene_builder.build_scene`.
"""

from __future__ import annotations

from pathlib import Path

import bpy
from bpy.props import BoolProperty, FloatProperty, IntProperty, StringProperty
from bpy.types import Context, Operator

from .scene_builder import BuildOptions, build_scene


class HIVEMIND_OT_build_plan_preview(Operator):
    """Load a Hivemind plan + intent and build an animated preview scene."""

    bl_idname = "hivemind.build_plan_preview"
    bl_label = "Build Plan Preview…"
    bl_options = {"REGISTER", "UNDO"}

    plan_filepath: StringProperty(
        name="Plan",
        subtype="FILE_PATH",
        description="Path to the HivemindPlan JSON file (oracle slicer output)",
    )
    intent_filepath: StringProperty(
        name="Intent",
        subtype="FILE_PATH",
        description="Path to the intent JSON (optional if embedded in plan)",
    )
    fps: IntProperty(
        name="FPS",
        default=24,
        min=1,
        max=120,
        description="Animation frame rate",
    )
    marker_radius: FloatProperty(
        name="Drone marker radius (m)",
        default=0.5,
        min=0.01,
        max=10.0,
    )
    sample_every_n_frames: IntProperty(
        name="Sample every N frames",
        default=1,
        min=1,
        max=60,
        description="Lower = denser keyframes, higher = lighter scene",
    )
    clear_scene: BoolProperty(
        name="Clear scene first",
        default=True,
        description="Delete all existing objects before importing",
    )

    def invoke(self, context: Context, event) -> set[str]:
        return context.window_manager.invoke_props_dialog(self, width=480)

    def draw(self, context: Context) -> None:
        layout = self.layout
        layout.prop(self, "plan_filepath")
        layout.prop(self, "intent_filepath")
        layout.separator()
        col = layout.column(align=True)
        col.prop(self, "fps")
        col.prop(self, "marker_radius")
        col.prop(self, "sample_every_n_frames")
        layout.prop(self, "clear_scene")

    def execute(self, context: Context) -> set[str]:
        if not self.plan_filepath:
            self.report({"ERROR"}, "Plan file is required")
            return {"CANCELLED"}

        plan_path = Path(bpy.path.abspath(self.plan_filepath))
        if not plan_path.is_file():
            self.report({"ERROR"}, f"Plan file not found: {plan_path}")
            return {"CANCELLED"}

        intent_path: Path | None = None
        if self.intent_filepath:
            intent_path = Path(bpy.path.abspath(self.intent_filepath))
            if not intent_path.is_file():
                self.report({"ERROR"}, f"Intent file not found: {intent_path}")
                return {"CANCELLED"}

        options = BuildOptions(
            fps=self.fps,
            drone_marker_radius=self.marker_radius,
            sample_every_n_frames=self.sample_every_n_frames,
            clear_scene=self.clear_scene,
        )

        try:
            result = build_scene(plan_path, intent_path, options)
        except Exception as exc:
            # Catch-all is intentional here: anything the builder raises
            # (file IO, JSON, value errors, bpy errors) should be surfaced to
            # the operator as an error report rather than crashing Blender.
            self.report({"ERROR"}, f"Build failed: {exc}")
            return {"CANCELLED"}

        for warning in result.warnings:
            self.report({"WARNING"}, warning)

        self.report(
            {"INFO"},
            (
                f"Built {result.walls} walls + {result.drones} drones, "
                f"{result.keyframes} keyframes over {result.total_duration_s:.1f}s"
            ),
        )
        return {"FINISHED"}


_classes = (HIVEMIND_OT_build_plan_preview,)


def register() -> None:
    for cls in _classes:
        bpy.utils.register_class(cls)


def unregister() -> None:
    for cls in reversed(_classes):
        bpy.utils.unregister_class(cls)
