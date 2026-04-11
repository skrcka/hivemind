# Hivemind

Remote control platform for swarms of industrial drones. Hivemind coordinates fleets of drones performing physical work on large structures — cleaning, painting, inspection — by combining 3D structure scans, mission planning, and live operator oversight.

## Overview

A human operator drives the system from a single control plane. They load a 3D scan of the target structure, plan the work, push the plan to the swarm, and supervise execution with the ability to intervene at any point. The drones execute autonomously where possible and fall back to operator control when needed.

## Submodules

### pantheon
Operator control plane. Long-term: Tauri + React desktop application. **v1: Blender + Skybrush Studio with custom Python plugins.**

This is where the human sits. Pantheon visualizes 3D scans of target structures, displays live swarm status, and exposes manual controls and behavior overrides for individual drones or the whole fleet. Mission plans are authored or reviewed here and handed off to oracle for execution.

For v1, "pantheon" is not a custom app — it is Blender (for 3D viz and path editing) + Skybrush Studio (for drone trajectory export and safety validation) + a small set of Hivemind Python plugins that add the industrial-painting bits (surface-following toolpaths, refill scheduling). The custom Tauri + React app is built later, once Blender's UX proves too clunky for field operators and the problem space is well understood.

### legion
Swarm communication and coordination layer.

Legion handles drone-to-drone communication, formation, and per-drone status reporting within the swarm. It receives per-drone instructions from oracle and is responsible for keeping the swarm coherent in the field — distributing commands, propagating state, and surfacing status back up the stack.

### vanguard
Standalone scout drone system (single drone for now, manually operated).

Vanguard's job is to fly the target structure and produce a 3D map of it. The resulting scan data is pulled into pantheon, where the operator analyzes it and builds the mission plan that the rest of the swarm will execute. Vanguard is intentionally separate from the worker swarm — different role, different lifecycle.

### oracle
Orchestrator and integration hub. Mix of hardware and software.

Oracle is the bridge between pantheon (intent) and legion (execution). It handles communication with all drones, ingests data from every part of the system, and uploads mission plans to the drones. Pantheon hands plans to oracle; oracle distributes them to the swarm via legion and routes telemetry back the other way.

## Data flow

```
vanguard ──(3D scan)──▶ pantheon ──(plan)──▶ oracle ──(per-drone instructions)──▶ legion ──▶ swarm
                            ▲                   │
                            └──(telemetry/status)┘
```

1. **vanguard** scans the structure.
2. **pantheon** ingests the scan, the operator authors a plan.
3. **pantheon** hands the plan to **oracle**.
4. **oracle** uploads per-drone instructions and brokers all drone communication.
5. **legion** runs the swarm in the field and reports status back through oracle to pantheon.

## Technology stack

Hivemind sits on top of the existing open-source drone ecosystem rather than reinventing it. The competitive moat lives in **pantheon** — the 3D-aware mission planning UI that turns "paint this bridge" into drone instructions. Everything below that is commodity infrastructure.

### Foundations: PX4 + MAVLink + MAVSDK

**PX4** is the firmware running on each drone's Pixhawk flight controller. It handles the low-level work: reading sensors, stabilizing flight, executing waypoints, motor mixing, PID loops. Hivemind does not write flight control code — PX4 does that. Hivemind sends it commands like "go to position X,Y,Z" or "follow this trajectory" and PX4 figures out the rest.

**MAVLink** is the communication protocol — the language drones speak. Every command and telemetry message between ground and drone is a MAVLink packet.

**MAVSDK-Python** is a Python library that wraps MAVLink into a clean async API. Oracle is essentially a service that manages multiple `System()` instances, one per drone.

```python
from mavsdk import System

drone = System()
await drone.connect(system_address="udp://:14540")

# arm and take off
await drone.action.arm()
await drone.action.takeoff()

# fly to a GPS position
await drone.action.goto_location(lat, lon, alt, yaw)

# or offboard mode for precise control
await drone.offboard.set_position_ned(position)
await drone.offboard.start()

# read telemetry
async for position in drone.telemetry.position():
    print(f"Lat: {position.latitude_deg}, Lon: {position.longitude_deg}")
```

**Skybrush** is the full swarm management layer — ground station UI, swarm coordination server, FlockWave protocol, and an ArduPilot firmware fork optimized for synchronized flight. Hivemind borrows from it where useful (oracle may extend Skybrush Server; pantheon may borrow components from Skybrush Live).

### Module-by-module: build vs. reuse

#### vanguard — build 10% / reuse 90%
A drone with a camera or LiDAR doing photogrammetry. Existing solutions cover almost the entire job:
- **Hardware:** any PX4 drone with a camera gimbal.
- **Flight planning:** QGroundControl runs survey/mapping missions out of the box.
- **3D reconstruction:** OpenDroneMap (open source) takes the photos and produces a 3D mesh, point cloud, and orthomosaic.
- **Small structures:** iPhone LiDAR is a viable alternative.

What Hivemind builds: the pipeline glue that takes ODM output and imports it into pantheon.

#### pantheon — v1: build 15% / reuse 85% · long-term: build 70% / reuse 30%
The most original work in the project. Nothing off-the-shelf does "load a 3D mesh of a bridge, let an operator paint regions on it, generate drone spray paths from those regions" — but **Blender + Skybrush Studio gets ~70% of the way there for free**, and Hivemind ships v1 on top of it.

**v1 — Blender + Skybrush Studio + Hivemind plugins.**
Skybrush Studio for Blender already does most of the heavy lifting:
- Imports 3D geometry (the bridge scan).
- Defines drone formations and trajectories in 3D space relative to the geometry.
- Validates collisions (minimum distance between all drones), velocity, and acceleration limits.
- Exports per-drone missions as `.skyc` with real-world GPS coordinates.
- Generates safety reports (PDF: nearest-neighbor distances, speed limits, etc.).
- Hands missions to Skybrush Server, which uploads them to the drones.

The pipeline becomes:

```
Bridge scan (OBJ/PLY)
  → Import into Blender
    → Hivemind plugin generates surface-following spray paths
      → Skybrush Studio validates and exports .skyc
        → Skybrush Server uploads to drones
          → Drones execute
```

Skybrush Studio was designed for "drones as flying pixels making shapes in the sky," not "drones as workers following a surface." The two gaps Hivemind fills as **Blender Python plugins**:

1. **Surface path generator** — takes the bridge mesh, generates parallel spray passes at a fixed standoff distance from the surface (think CNC toolpath, but in 3D), and emits Skybrush-compatible drone trajectories.
2. **Refill scheduler** — splits long paths into per-load segments based on paint capacity (e.g. 500 g per drone) and inserts return-to-base waypoints between segments.

Sketch of the plugin pattern:

```python
import bpy
from bpy.props import FloatProperty
from skybrush_studio import StoryboardEntry

class GenerateSprayPaths(bpy.types.Operator):
    bl_idname = "hivemind.generate_spray_paths"
    bl_label = "Generate Spray Paths"

    standoff_distance: FloatProperty(default=0.5)  # meters from surface
    spray_width: FloatProperty(default=0.3)        # meters per pass
    paint_capacity: FloatProperty(default=0.5)     # kg per drone load

    def execute(self, context):
        bridge_mesh = context.active_object
        # generate parallel passes offset from surface
        # split into per-drone segments based on paint capacity
        # create Skybrush storyboard entries
        return {'FINISHED'}
```

For live operations, **Skybrush Live** (the web GCS) is used as-is — it shows all drones on a map with status, and operators can pause/abort from there. Telemetry is *not* overlaid on the 3D bridge model in v1; that comes with custom pantheon.

**Long-term — custom Tauri + React app.**
Blender is not a field-ops UX — truck operators should not be learning Blender. Once the v1 pipeline has proven the workflow and the problem space is well understood, pantheon becomes a purpose-built app. Existing pieces to lean on at that point:
- **Three.js / React Three Fiber** for 3D mesh visualization inside the Tauri app.
- **QGroundControl** source as a reference for telemetry display patterns (not embedded).
- **Skybrush Live** (React/TypeScript) — open source GCS frontend, individual map/telemetry components can be borrowed.

What custom pantheon eventually owns: the 3D structure viewer, the mission painting and planning UI, the plan-to-drone-path compiler (ported out of the Blender plugin), and a live telemetry overlay on the 3D bridge model. **This is the core product moat.** v1 exists to learn the problem; v2 exists to win on UX.

**v1 vs. custom pantheon at a glance:**

| Capability | v1 (Blender + Skybrush Studio) | Custom pantheon (later) |
|---|---|---|
| 3D scan visualization | Blender (world-class) | Three.js / R3F |
| Drone path editing | Skybrush Studio (keyframe, visual) | Built from scratch |
| Collision checking | Skybrush Studio (built in) | Built from scratch |
| Safety validation | Skybrush Studio (speed/accel/distance) | Built from scratch |
| Mission export to drones | Skybrush Server (`.skyc`) | Built on MAVSDK |
| Surface-following paths | **Hivemind Blender plugin** | Built from scratch |
| Refill / rotation scheduling | **Hivemind Blender plugin** | Built from scratch |
| Live ops dashboard | Skybrush Live (2D map) | 3D-overlaid telemetry, custom |
| Field-ready UX | No — Blender is complex | Yes — purpose-built for operators |

#### oracle — build 30% / reuse 70%
Maps closely onto **Skybrush Server**: a Python backend that manages drone connections, distributes missions, aggregates telemetry, and handles RTK corrections.

Two viable paths:
- **Extend Skybrush Server** with paint-specific mission logic.
- **Roll a service in Python (or Rust)** on top of MAVSDK-Python or pymavswarm for full control.

The oracle↔drone link is just MAVLink over radio/WiFi — MAVSDK handles it. What Hivemind adds is the business logic: mission decomposition (turning "paint this 50m² area" into per-drone waypoint sequences), drone rotation scheduling (who breaks off to refill paint and when), and routing telemetry back to pantheon.

#### legion — build 20% / reuse 80%
Two architectural options:

**Approach A — Centralized (recommended for v1).**
There is no "legion" as a separate runtime layer. Oracle sends each drone its complete mission. Drones execute independently (like drone shows). Coordination reduces to "don't collide," which is solved at planning time by deconflicting paths. This is how drone shows work and it scales to thousands of drones.

**Approach B — Distributed (v2+).**
Drones talk to each other, negotiate, adapt in flight. Significantly harder, and unnecessary for bridge painting where the structure doesn't move and the work is predictable. Defer.

For v1, "legion" is effectively pre-planned PX4 offboard paths with collision deconfliction baked into the plan compiler in pantheon.

### Summary table

| Module | Existing tool | Hivemind code | Notes |
|---|---|---|---|
| **vanguard** | QGroundControl + OpenDroneMap | Import pipeline | Mostly solved |
| **pantheon (v1)** | Blender + Skybrush Studio + Skybrush Live | Surface-path & refill-scheduler Blender plugins | Ship on this; learn the problem |
| **pantheon (later)** | Three.js, Skybrush Live components | 3D planning UI, mission compiler, live 3D telemetry | **Core product moat** |
| **oracle** | Skybrush Server or MAVSDK-Python | Mission decomposition, scheduling | Extend or build on existing |
| **legion** | PX4 offboard mode + pre-planned paths | Collision deconfliction at plan time | Skip as separate layer for v1 |
| **drone firmware** | PX4 (unmodified) | — | Don't touch |
| **comms protocol** | MAVLink (+ FlockWave if using Skybrush) | — | Don't reinvent |

### Concrete architecture

**v1 — Blender-based pantheon:**

```
                    EXISTING + HIVEMIND PLUGINS                EXISTING
               ┌──────────────────────────────┐     ┌──────────────────────┐
  iPhone /     │  Blender + Skybrush Studio   │     │                      │
  ODM scan ──▶ │  + Hivemind plugins:         │.skyc│  Skybrush Server     │
               │    - surface path generator  │────▶│  (mission upload,    │
               │    - refill scheduler        │     │   telemetry routing) │
               │                              │     │                      │
               │  Live ops: Skybrush Live     │◀────│                      │
               │  (2D map, pause/abort)       │telem└──────────┬───────────┘
               └──────────────────────────────┘                │ MAVLink
                                                      ┌────────┼────────┐
                                                      ▼        ▼        ▼
                                                   Drone 1  Drone 2  Drone 3
                                                   (PX4)    (PX4)    (PX4)
```

**Long-term — custom pantheon:**

```
                    HIVEMIND CODE                       EXISTING
               ┌─────────────────┐          ┌──────────────────────┐
  iPhone /     │   pantheon      │          │                      │
  ODM scan ──▶ │   (Tauri+React) │──plan──▶ │  oracle              │
               │   3D viewer     │          │  (Python service)    │
               │   mission paint │◀─telem── │  MAVSDK-Python       │
               │   operator UI   │          │  or Skybrush Server  │
               └─────────────────┘          └──────────┬───────────┘
                                                       │ MAVLink
                                              ┌────────┼────────┐
                                              ▼        ▼        ▼
                                           Drone 1  Drone 2  Drone 3
                                           (PX4)    (PX4)    (PX4)
```

The migration path: the Blender plugins' surface-path and refill-scheduler logic ports almost directly into the custom pantheon as the plan compiler. v1 isn't throwaway — it's a working system *and* a spec for v2.

## Spatial alignment (zeroing)

The hardest non-obvious problem in Hivemind is **registration**: aligning the 3D scan to real-world GPS so that "fly to point X on the mesh" maps to actual lat/lon/alt with centimeter accuracy. The 3D scan lives in arbitrary "mesh space" (origin at 0,0,0 wherever the scan started); the drones navigate in GPS. A few centimeters of misalignment is the difference between *painted the bridge* and *painted the river*.

Drone shows largely sidestep this — their "structure" is empty sky, so they just pick a GPS origin, orient north, and use relative offsets. Hivemind has to align to a physical surface, so it has to solve registration for real.

### Approach 1 — Ground Control Points (GCPs)

Standard surveying technique. Use this when the scan is not georeferenced from the start.

1. **Place physical markers** (printed ArUco targets or survey targets) at 4–6 spread-out locations on the structure.
2. **Measure their GPS positions** with an RTK GPS unit at centimeter accuracy. The truck's RTK base station provides corrections.
3. **Identify the same points in the 3D scan** — manually click them in Blender, or auto-detect ArUco markers in the scan photos.
4. **Compute the transform.** With 4+ point correspondences between mesh-space and GPS-space, solve for a rigid transform (rotation + translation, optionally scale) using the Kabsch algorithm:

```python
import numpy as np

def compute_alignment(mesh_points, gps_points):
    """
    mesh_points: Nx3 array of points in scan coordinates
    gps_points:  Nx3 array of same points in local ENU coordinates
    Returns rotation matrix R and translation vector t.
    """
    mesh_centroid = mesh_points.mean(axis=0)
    gps_centroid = gps_points.mean(axis=0)

    mesh_centered = mesh_points - mesh_centroid
    gps_centered = gps_points - gps_centroid

    # Kabsch algorithm: SVD for the optimal rotation
    H = mesh_centered.T @ gps_centered
    U, S, Vt = np.linalg.svd(H)
    R = Vt.T @ U.T

    # Fix reflection if needed
    if np.linalg.det(R) < 0:
        Vt[-1, :] *= -1
        R = Vt.T @ U.T

    t = gps_centroid - R @ mesh_centroid
    return R, t
```

After this, every point on the mesh has a GPS coordinate. The path planner generates paths in mesh space, applies the transform, and outputs GPS waypoints.

### Approach 2 — Georeferenced scan (preferred)

If vanguard does the scanning with **RTK GPS running**, every photo or LiDAR frame is already GPS-tagged. OpenDroneMap consumes the EXIF tags and produces a georeferenced model automatically — the mesh comes out in real-world coordinates from the start, no manual alignment required.

```
Scout drone (RTK GPS + camera)
  → Photos with GPS EXIF tags
    → OpenDroneMap (georeferenced reconstruction)
      → Mesh already in real-world coordinates
        → Path planner generates GPS waypoints directly
```

This is the cleanest pipeline and the default Hivemind targets. GCPs become a fallback for scans done without RTK (e.g. iPhone LiDAR on small structures).

### At-job-site zeroing

A georeferenced scan is necessary but not sufficient. Absolute GPS drifts, and 10 cm of drift means paint on the wrong spot. Three mechanisms run together at the job site:

- **RTK base station on the truck.** A fixed GPS antenna on the truck broadcasts corrections to every drone. Even if the absolute GPS reference is off, all drones share the *same* reference, so relative positions stay consistent to ~2 cm. **This is the single most important piece of hardware after the drones themselves.** Without it: ±2 m accuracy. With it: ±2 cm. Skybrush Server already supports RTK distribution to the swarm.
- **Visual alignment check before each job.** Before the swarm starts work, fly one drone to a known reference point on the structure (a GCP, or any landmark with a known GPS position) and verify it's actually where the system thinks it is. If it's off by 5 cm, apply a correction offset to the entire mission. This is the equivalent of a CNC machine touching off on a reference point before cutting.
- **Surface-relative sensing during operation.** Each drone carries a downward- or forward-facing distance sensor (ultrasonic or ToF). Instead of relying purely on absolute GPS altitude (which has worse error than horizontal), the drone maintains a fixed standoff from the surface itself. This handles GPS altitude drift and surface irregularities the scan missed.

### Full alignment pipeline

```
                    BEFORE JOB (office)
┌─────────────────────────────────────────────┐
│  1. Scout drone scans bridge (RTK GPS on)   │
│  2. OpenDroneMap → georeferenced 3D mesh    │
│  3. Plan spray paths in Blender/Skybrush    │
│  4. Export missions with GPS waypoints      │
└─────────────────────────────────────────────┘

                    AT JOB SITE (truck)
┌─────────────────────────────────────────────┐
│  5. Set up RTK base station on truck        │
│  6. Fly one drone to a known point on the   │
│     bridge → verify alignment               │
│  7. Apply correction offset if needed       │
│  8. Start swarm, each drone uses:           │
│     - RTK GPS for horizontal position       │
│     - ToF sensor for surface standoff       │
└─────────────────────────────────────────────┘
```

### Implications for the submodules

- **vanguard** must fly with RTK GPS active during scans. Photos/LiDAR frames need GPS EXIF tags. The import pipeline into pantheon assumes a georeferenced mesh and falls back to GCP-based alignment only when EXIF is missing.
- **pantheon** needs GCP marking and the Kabsch transform as a tool for non-georeferenced scans. The mission compiler always emits GPS waypoints, never mesh-space coordinates. v1 lives in Blender, where ArUco identification and point picking are straightforward Python plugin work.
- **oracle** distributes RTK corrections to the swarm (or delegates to Skybrush Server, which already does this) and exposes the pre-flight alignment-check workflow: "fly drone N to known point P, read back actual GPS, compute and apply mission offset."
- **drones** carry an RTK GPS receiver and a ToF/ultrasonic distance sensor for surface-relative standoff. PX4 already supports both via standard sensor drivers — no firmware changes needed.
