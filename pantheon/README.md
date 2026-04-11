# pantheon

The operator-facing tool. Pantheon is where a human authors *what should be painted* on a 3D scan of the structure. It does not generate drone paths, schedule sorties, or talk to the swarm — those are oracle's job. Pantheon's only output is an **intent file**: a list of regions on the mesh that the operator wants painted, plus a paint spec and any constraints.

> See the top-level [README](../README.md) for project context. Pantheon's output is consumed by oracle — see [oracle/README.md → The Plan](../oracle/README.md#the-plan) for how the intent fits into oracle's plan/apply lifecycle.

## Role

Pantheon is the CAD tool of the system. Oracle is the slicer.

| Concern | Pantheon | Oracle |
|---|---|---|
| 3D model viewing and editing | ✓ | — |
| Operator marks "paint these surfaces" | ✓ | — |
| Paint spec (type, thickness, coats) | ✓ (intent metadata) | ✓ (consumed by slicer) |
| Time/weather/no-fly constraints | ✓ (intent metadata) | ✓ (consumed by slicer) |
| Generate per-drone spray paths | — | ✓ |
| Decide drone assignments | — | ✓ |
| Schedule refills | — | ✓ |
| Live fleet monitoring | — | ✓ (but pantheon *displays* it) |
| Talk to drones | — | ✓ |
| Approve / reject plans | ✓ (operator UI) | ✓ (the plan/apply gate) |

The boundary is sharp on purpose. Pantheon never sees per-drone state and never compiles paths, because those depend on **live fleet facts** (battery, paint, weather, RTK status, what's already been painted on a replan) that pantheon does not have. See [oracle's slicer rationale](../oracle/README.md#the-core-insight-oracle-is-the-slicer) — same idea, different end of the wire.

## Phases

Pantheon ships in two phases. v1 is intentionally minimal — three operations and an export format. The custom app comes later, once v1 has proven the workflow and the problem space is well understood.

| Phase | Implementation | Scope |
|---|---|---|
| **v1** | Blender add-on (Hivemind Plane Picker) | Open mesh → mark regions → export intent JSON |
| **Long-term** | Custom Tauri + React desktop app | Same flow + live telemetry overlay + plan review UI + operator field-ops UX |

Both phases emit the **same intent file format**, so oracle does not care which one produced it. The custom app is a UX upgrade, not an API change.

## v1 — Blender add-on

### v1 scope

Three things, no more:

1. **Open a 3D model.** Bridge mesh as `.obj`, `.ply`, or `.stl`. Optionally georeferenced (mesh-space coordinates already aligned to GPS — see [main README → Spatial alignment](../README.md#spatial-alignment-zeroing)).
2. **Select planes for painting.** The operator picks faces in the mesh and groups them into named regions ("north face main span," "south arch underside"). Multiple regions per scan.
3. **Export the intent.** Write a JSON file (or directory) describing the regions. Oracle ingests this in its plan phase (`hivemind plan --intent intent.json`).

That's the entire v1 feature set. No path generation, no drone simulation, no preview animation, no telemetry. Those either live in oracle or come in the long-term app.

### v1 user flow

```
1. Open Blender
2. File → Import → Wavefront (.obj)            ← bridge mesh from vanguard / OpenDroneMap
3. Switch to Edit Mode → Face select
4. Select faces of the first surface to paint
5. N-panel → Hivemind tab → "Mark Selected as Region"
6. Type a region name (e.g. "north-face-main-span")
7. Repeat steps 3–6 for every surface to paint
8. Hivemind tab → "Export Intent…" → choose path → save intent.json
9. Hand intent.json to oracle:
       $ hivemind plan --intent intent.json --drones 6
```

That's the whole loop. The operator is doing two things in Blender that Blender already does well — viewing a 3D mesh and selecting faces — plus two custom buttons added by the Hivemind add-on.

### Why Blender for v1

- **3D viewing is solved.** Blender is world-class at importing, rotating, lighting, and inspecting meshes. Building this in a custom app means months of Three.js work for no gain in v1.
- **Face selection is built in.** Blender's edit mode + face-select tool already handles click-to-select, box-select, lasso, "select linked," and "select coplanar." All of those are tools the operator wants when picking paint regions.
- **Python API is excellent.** Blender's `bpy` API gives the add-on direct access to the active mesh, the current selection, and the UI panel system. The whole add-on is a few hundred lines.
- **Free and cross-platform.** No license cost, runs on every dev machine.

The downside is that Blender is not a field-ops UX — truck operators shouldn't have to learn Blender. That's exactly what the long-term custom app fixes. But for v1, the operator authoring intent is *us*, not the truck operator. We can use Blender.

### Add-on structure

The Hivemind Plane Picker is a single Blender add-on registered as `hivemind_plane_picker`. It contributes:

- **A panel** in the 3D viewport's N-panel under a "Hivemind" tab.
- **Two operators**:
  - `hivemind.mark_region` — takes the current face selection, prompts for a region name, stores it as a vertex group + custom property on the mesh.
  - `hivemind.export_intent` — walks every region marked on the active mesh, computes geometry + area + bounding box for each, and writes the intent JSON.
- **A region list** in the panel showing every marked region on the active mesh, with rename / re-select / delete actions.

Sketch:

```python
import bpy
import json
from bpy.props import StringProperty
from mathutils import Vector

bl_info = {
    "name": "Hivemind Plane Picker",
    "blender": (4, 0, 0),
    "category": "Object",
}

# ─── Operators ──────────────────────────────────────────────────────

class HIVEMIND_OT_mark_region(bpy.types.Operator):
    """Mark the current face selection as a paint region."""
    bl_idname = "hivemind.mark_region"
    bl_label = "Mark Selected as Region"

    region_name: StringProperty(name="Region name", default="region")

    def invoke(self, context, event):
        return context.window_manager.invoke_props_dialog(self)

    def execute(self, context):
        obj = context.active_object
        if obj is None or obj.type != 'MESH':
            self.report({'ERROR'}, "Select a mesh in Edit Mode first")
            return {'CANCELLED'}

        # Snapshot the selected face indices into a custom property
        bpy.ops.object.mode_set(mode='OBJECT')
        selected_faces = [f.index for f in obj.data.polygons if f.select]
        bpy.ops.object.mode_set(mode='EDIT')

        if not selected_faces:
            self.report({'ERROR'}, "No faces selected")
            return {'CANCELLED'}

        regions = obj.get("hivemind_regions", {})
        regions[self.region_name] = list(selected_faces)
        obj["hivemind_regions"] = regions

        self.report({'INFO'}, f"Marked {len(selected_faces)} faces as '{self.region_name}'")
        return {'FINISHED'}


class HIVEMIND_OT_export_intent(bpy.types.Operator):
    """Export all marked regions as a Hivemind intent JSON."""
    bl_idname = "hivemind.export_intent"
    bl_label = "Export Intent…"

    filepath: StringProperty(subtype='FILE_PATH')

    def invoke(self, context, event):
        context.window_manager.fileselect_add(self)
        return {'RUNNING_MODAL'}

    def execute(self, context):
        obj = context.active_object
        regions = obj.get("hivemind_regions", {})

        intent = {
            "version": "1.0",
            "scan": {
                "id": obj.name,
                "source_file": bpy.data.filepath,
                "georeferenced": obj.get("georeferenced", False),
            },
            "regions": [],
            "constraints": {},
        }

        for name, face_indices in regions.items():
            faces_out = []
            total_area = 0.0
            for fi in face_indices:
                f = obj.data.polygons[fi]
                verts = [tuple(obj.matrix_world @ obj.data.vertices[v].co) for v in f.vertices]
                normal = tuple(obj.matrix_world.to_3x3() @ f.normal)
                faces_out.append({"vertices": verts, "normal": normal})
                total_area += f.area

            intent["regions"].append({
                "id": name,
                "name": name,
                "faces": faces_out,
                "area_m2": total_area,
            })

        with open(self.filepath, 'w') as f:
            json.dump(intent, f, indent=2)

        self.report({'INFO'}, f"Wrote {len(intent['regions'])} regions to {self.filepath}")
        return {'FINISHED'}


# ─── Panel ──────────────────────────────────────────────────────────

class HIVEMIND_PT_panel(bpy.types.Panel):
    bl_label = "Hivemind"
    bl_space_type = 'VIEW_3D'
    bl_region_type = 'UI'
    bl_category = "Hivemind"

    def draw(self, context):
        layout = self.layout
        layout.operator("hivemind.mark_region", icon='ADD')

        obj = context.active_object
        if obj and "hivemind_regions" in obj:
            layout.label(text="Regions:")
            for name, faces in obj["hivemind_regions"].items():
                layout.label(text=f"  {name} ({len(faces)} faces)")

        layout.separator()
        layout.operator("hivemind.export_intent", icon='EXPORT')


# ─── Registration ───────────────────────────────────────────────────

classes = (HIVEMIND_OT_mark_region, HIVEMIND_OT_export_intent, HIVEMIND_PT_panel)

def register():
    for c in classes: bpy.utils.register_class(c)

def unregister():
    for c in classes: bpy.utils.unregister_class(c)
```

The above is illustrative — the actual implementation lives in this directory and is split into a few cleanly-separated modules (see [Implementation](#implementation) below). The shape is identical, just organized for testability and registration order.

### Implementation

The add-on is implemented in `pantheon/hivemind_plane_picker/` as a Blender 4.2+ extension package (compatible with Blender 5.x). The directory layout:

```
pantheon/
├── Makefile                            # install / test / lint / build / clean
├── pyproject.toml                      # ruff config + project metadata
├── hivemind_plane_picker/
│   ├── blender_manifest.toml           # extension manifest
│   ├── __init__.py                     # registration entry point
│   ├── properties.py                   # PropertyGroups stored on bpy.types.Mesh
│   ├── intent.py                       # pure intent-document builder (no bpy)
│   ├── operators.py                    # bpy.types.Operator subclasses
│   └── ui.py                           # Panel + UIList in the 3D viewport
└── tests/
    └── test_intent.py                  # unit tests for intent.py (run with stock python3)
```

The split is deliberate:

- **`intent.py` is the only module that has no `bpy` dependency.** It owns the v1.0 schema, the `Face` / `Region` / `Scan` dataclasses, and the `build_intent` / `write_intent` functions. Because it's pure data, it's unit-tested in isolation by `tests/test_intent.py` against stock Python — no Blender process required, no GUI. This is the module that defines the contract with oracle.
- **`operators.py` is where bpy meets the pure logic.** Each operator reads state from Blender (selected faces, the active mesh, the object's world transform), converts it to plain Python data, calls into `intent.py`, and reports the result. The triangulation, world-space transform, and area calculation live in a small helper (`_build_region_payload`) that's the only place bpy and pure logic meet.
- **`properties.py` defines the storage shape.** Regions are stored on `bpy.types.Mesh` (not `Object`) so duplicating an object that shares the mesh keeps the regions consistent across instances. Region face indices are stored as a sub-collection of `HivemindFaceIndex` PropertyGroups because Blender's `CollectionProperty` cannot hold raw integers.
- **`ui.py` is just layout.** A `UIList` for the region list and a single `Panel` in the 3D viewport's N-panel under the "Hivemind" tab.

Operators implemented:

| Operator | bl_idname | What it does |
|---|---|---|
| Mark Selected as Region | `hivemind.mark_region` | Adds the current face selection as a new named region |
| Select | `hivemind.select_region` | Replaces the current face selection with the active region's faces |
| Rename | `hivemind.rename_region` | Renames the active region (validates uniqueness) |
| Remove | `hivemind.remove_region` | Removes the active region from the list |
| Clear All Regions | `hivemind.clear_regions` | Removes every region (with confirmation) |
| Export Intent… | `hivemind.export_intent` | Triangulates every region's faces, computes world-space normals + area, writes `intent.json` via `ExportHelper` |

### Install for development

The Makefile symlinks the add-on into Blender 5.1's user extensions directory so edits are picked up without re-copying:

```bash
cd pantheon
make install
```

This creates a symlink at:
- macOS: `~/Library/Application Support/Blender/5.1/extensions/user_default/hivemind_plane_picker`
- Linux: `~/.config/blender/5.1/extensions/user_default/hivemind_plane_picker`

(Windows: install manually via `Edit → Preferences → Get Extensions → Install from Disk → pantheon/hivemind_plane_picker`.)

Then in Blender:

```
Edit → Preferences → Add-ons → enable "Hivemind Plane Picker"
```

The panel appears in any 3D viewport: press `N` to open the side panel, switch to the **Hivemind** tab.

### Other dev tasks

```bash
make test       # run the intent.py unit tests with stock python3 (no Blender)
make lint       # ruff check
make format     # ruff format
make build      # produce dist/hivemind_plane_picker-<version>.zip for distribution
make uninstall  # remove the symlink
make clean      # remove caches and build artifacts
```

The tests run against pure-Python and validate the intent file format end-to-end (build, write, round-trip, UTF-8, schema version, constraints handling). They are the safety net for any future change to the schema.

### v1+ niceties (optional)

Things that are easy to add once the core flow works, but that v1.0 can ship without:

- **Visual region highlight** — colour each marked region with a different material so the operator can see what's already been picked.
- **Coplanar face auto-select** — "select all faces coplanar with this one" operator. Useful for picking flat bridge sides without box-selecting every triangle.
- **Per-region paint spec** — let the operator set paint type / thickness / coats per region in the panel, instead of relying on a global default.
- **Region area sanity check** — flag regions smaller than X m² or larger than Y m² as warnings before export.
- **Re-import existing intent.json** — load a previous intent into Blender, repaint the regions on the mesh, edit and re-export.

None of these change the export format. They just make the v1 add-on more pleasant to use.

## Intent file format (v1.0)

The intent file is the contract between pantheon and oracle. Both phases of pantheon (Blender and the long-term app) emit the same format, and oracle parses it the same way regardless of source.

```json
{
  "version": "1.0",
  "scan": {
    "id": "north-bridge-2026-04-11",
    "source_file": "/path/to/bridge.obj",
    "georeferenced": true
  },
  "regions": [
    {
      "id": "north-face-main-span",
      "name": "North face — main span",
      "faces": [
        {
          "vertices": [
            [12.34, 56.78, 9.01],
            [12.50, 56.78, 9.01],
            [12.50, 56.92, 9.01]
          ],
          "normal": [0.0, 0.0, 1.0]
        }
      ],
      "area_m2": 12.4
    }
  ],
  "constraints": {}
}
```

### Field reference

| Field | Type | Required | Notes |
|---|---|---|---|
| `version` | string | ✓ | Schema version. v1.0 = "1.0". |
| `scan.id` | string | ✓ | Stable identifier for the scan, used by oracle for replan/resume continuity. |
| `scan.source_file` | string | — | Path to the original mesh, for traceability. |
| `scan.georeferenced` | bool | ✓ | If true, vertex coordinates are in real-world units (e.g. local ENU around the RTK base origin). If false, coordinates are mesh-space and oracle will require a GCP alignment step. |
| `regions[].id` | string | ✓ | Stable identifier; replans reference regions by id. |
| `regions[].name` | string | ✓ | Human-readable name shown in oracle's plan preview. |
| `regions[].faces[].vertices` | `[[x,y,z], [x,y,z], [x,y,z]]` | ✓ | Triangle vertices in world coordinates. v1 emits triangles only (Blender exports n-gons as triangle fans). |
| `regions[].faces[].normal` | `[x,y,z]` | ✓ | Outward face normal — tells oracle which side to spray from. |
| `regions[].area_m2` | float | ✓ | Pre-computed surface area in square metres. Oracle uses this for paint volume estimation. |
| `regions[].paint_spec` | object | — | Optional per-region paint spec. If absent, oracle uses the global default. v1.0 leaves this null. |
| `constraints` | object | — | Operator-set constraints (time window, no-fly zones, max simultaneous drones). v1.0 leaves this empty. |

### Why JSON, not OBJ + manifest

A more "correct" format would be a directory: one OBJ file per region plus a manifest. JSON-with-inline-geometry is more compact for small region counts, easier for oracle to parse in one shot, and easier to inspect by hand. For bridges with 50+ regions or millions of faces this won't scale, but v1 doesn't need to scale — it needs to be debuggable.

If a future version needs the OBJ-per-region split, the manifest stays JSON and inlined `faces` becomes a `mesh_file` reference. That's a forward-compatible change.

## Long-term — custom Tauri + React app

Blender is the right choice for v1 *us*. It is the wrong choice for a truck operator paid to push paint, not learn 3D modelling software. The long-term pantheon is a purpose-built desktop app:

- **Tauri shell** — single binary, runs on Windows / macOS / Linux, ~10 MB instead of an Electron blob.
- **React + Three.js / React Three Fiber** for the 3D mesh viewer. Borrow component patterns from Skybrush Live where useful.
- **Same intent file format.** The data contract with oracle does not change.
- **Same three core operations** (open scan, mark regions, export intent), polished into a workflow a non-technical operator can run with five minutes of training.

What the long-term app *adds* on top of v1's scope:

- **Plan review UI** — pantheon receives `HivemindPlan` objects from oracle (see [oracle's plan/apply lifecycle](../oracle/README.md#plan--apply-lifecycle)) and renders them: 3D bridge view with spray paths drawn on the surface colour-coded by drone, timeline scrubber, summary stats, warnings, approve/modify/reject buttons. This is the UI shown in [oracle's review mockup](../oracle/README.md#what-pantheon-shows-during-review).
- **Live telemetry overlay** — drone positions and sortie progress drawn on the *same* 3D bridge model the operator authored against. Skybrush Live shows drones on a 2D map; this is the upgrade to 3D-on-the-actual-structure.
- **Mid-execution control** — pause / abort / pause-region / skip-region buttons wired to oracle's amendment API.
- **Field-ops UX** — sunlight-readable contrast, large touch targets for the rugged tablet, no Blender-style modal mode switching.

What the long-term app does **not** add: anything that should be oracle's job. The slicer stays in oracle. The fleet monitor stays in oracle. Pantheon stays a pure client.

### Migration plan

The migration is a UI swap, not a rewrite, because the data contract doesn't change:

1. v1 ships as a Blender add-on emitting `intent.json`.
2. Custom app v0 ships as a desktop binary that emits the same `intent.json`. For a release or two, both pantheons coexist — the operator can use whichever they prefer.
3. The custom app adds plan review and live telemetry (the things Blender genuinely cannot do well).
4. Once the custom app has feature parity for authoring + adds the things Blender can't, the Blender add-on becomes a developer tool (kept around for debugging the intent format).

At no point does oracle have to care which pantheon is on the other end of the wire.

## What pantheon does *not* do

Worth being explicit, because the temptation is to put more here than belongs:

- **No path generation.** Spray paths, lane assignment, return-to-base routing — all oracle's job. Pantheon emits regions; oracle slices them.
- **No drone-aware simulation.** Pantheon never knows how many drones are online or what battery state they're in. That's why oracle (which does) is the slicer.
- **No direct drone communication.** Pantheon talks to oracle. Oracle talks to drones. The boundary is enforced.
- **No mid-flight authority.** Pantheon shows live telemetry and offers approve / pause / abort buttons; the actual decisions and commands originate in oracle.
- **No file format conversion.** If the scan isn't a mesh Blender can import, that's a vanguard / import-pipeline problem, not a pantheon problem.

The rule of thumb: pantheon is the *what*, oracle is the *how*. If a feature is about drones, fleet, or execution, it belongs in oracle. If it's about the operator's view of the structure and intent, it belongs in pantheon.
