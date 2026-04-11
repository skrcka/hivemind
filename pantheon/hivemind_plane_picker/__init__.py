"""Hivemind Plane Picker — Blender add-on entry point.

Lets the operator open a 3D scan of a structure, mark face selections as named
"regions" to be painted, and export them as a Hivemind intent file for
downstream processing by oracle.

Module layout:

    properties.py  — PropertyGroups stored on bpy.types.Mesh
    intent.py      — Pure intent-document builder (no bpy dependency, testable)
    operators.py   — bpy.types.Operator subclasses (the actual UI actions)
    ui.py          — Panel + UIList in the 3D viewport's N-panel
"""

from . import operators, properties, ui

# Register order matters: properties contribute the data shape that operators
# and the UI both depend on. Unregister in reverse order to tear down cleanly.
_modules = (properties, operators, ui)


def register() -> None:
    for module in _modules:
        module.register()


def unregister() -> None:
    for module in reversed(_modules):
        module.unregister()
