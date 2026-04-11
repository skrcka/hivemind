"""Hivemind Plan Preview — Blender add-on entry point.

Loads a HivemindPlan JSON (oracle's slicer output) plus the corresponding
pantheon intent JSON, and builds a Blender scene with:

- one mesh object per intent region (the walls);
- one marker per drone, named after its drone_id;
- location keyframes that move each marker through its sortie's waypoints
  over the plan's wall-clock duration.

Module layout:

    coords.py        — pure: GPS ↔ local ENU conversion
    plan_loader.py   — pure: parse plan + intent JSON into dataclasses
    timeline.py      — pure: position-of-drone-at-time interpolation
    scene_builder.py — bpy: build the Blender scene from parsed data
    operators.py     — bpy: in-Blender operator (file picker + button)
    ui.py            — bpy: panel section in the Hivemind N-panel
    cli.py           — standalone CLI for `blender --background --python`

The bpy-using submodules (operators, ui, scene_builder) are imported lazily
inside `register()` so the pure submodules (coords, plan_loader, timeline)
can be imported in stock Python for unit testing without bpy installed.
"""


def register() -> None:
    from . import operators, ui

    operators.register()
    ui.register()


def unregister() -> None:
    from . import operators, ui

    ui.unregister()
    operators.unregister()
