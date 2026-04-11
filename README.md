# Hivemind

Remote control platform for swarms of industrial drones. Hivemind coordinates fleets of drones performing physical work on large structures — cleaning, painting, inspection — by combining 3D structure scans, mission planning, and live operator oversight.

## Overview

A human operator drives the system from a single control plane. They load a 3D scan of the target structure, plan the work, push the plan to the swarm, and supervise execution with the ability to intervene at any point. The drones execute autonomously where possible and fall back to operator control when needed.

The same drone platform supports two **swappable payload modules** — paint and pressure-wash — selected per-drone via legion config at startup. Frame, motors, Pixhawk, Pi, and the entire ground stack are identical between them; only the bottom-plate hardware differs. v1 ships the paint module to validate the flight loop; the wash module is designed and documented for v2 deployment ([hw/wash](hw/wash/README.md)).

## Submodules

### pantheon
Operator control plane. Long-term: Tauri + React desktop application. **v1: small Blender add-on (Hivemind Plane Picker).**

Pantheon is the **CAD tool** of the system — it is where the operator opens a 3D scan of the structure, picks the surfaces to be painted, and exports an *intent file* for oracle. It does not generate drone paths, schedule sorties, or talk to drones — those are oracle's job. The boundary is intentionally sharp: pantheon = *what to paint*, oracle = *how to fly it*.

For v1, pantheon is a ~100-line Blender add-on with three operations: import a mesh, mark face selections as named regions, export `intent.json`. The custom Tauri + React app comes later, once v1 has proven the workflow and the problem space is well understood — at which point pantheon also gains a plan review UI and a live telemetry overlay on the 3D model. The intent file format stays the same across phases, so the migration is a UI swap, not a rewrite.

Detailed design, the v1 add-on code, and the intent file schema in [pantheon/README.md](pantheon/README.md).

### legion
On-drone agent. One instance per drone.

Legion is **not** a swarm mesh. There is no drone-to-drone communication anywhere in Hivemind — every drone talks only to oracle, in a star topology. Legion is the small agent that runs on each drone's companion computer (a Raspberry Pi alongside the Pixhawk). It receives sortie commands from oracle, drives PX4 to execute them, reports telemetry back to oracle, and runs a **local safety loop** (wall avoidance via ToF, oracle-heartbeat watchdog, low-battery RTL, paint-empty RTL) that protects the drone independently of oracle if the link drops. See [oracle/README.md](oracle/README.md#safety-and-deconfliction) for the full three-layer safety model.

### vanguard
Standalone scout drone system (single drone for now, manually operated).

Vanguard's job is to fly the target structure and produce a 3D map of it. The resulting scan data is pulled into pantheon, where the operator analyzes it and builds the mission plan that the rest of the swarm will execute. Vanguard is intentionally separate from the worker swarm — different role, different lifecycle.

### oracle
Orchestrator and integration hub. Mix of hardware and software.

Oracle is the bridge between pantheon (intent) and legion (execution). It handles communication with all drones, ingests data from every part of the system, and uploads mission plans to the drones. Pantheon hands plans to oracle; oracle distributes them to the swarm via legion and routes telemetry back the other way. Detailed design in [oracle/README.md](oracle/README.md).

### hw
Hardware specification — frames, flight controllers, payloads, ground station, RTK, refill. Split by phase: [hw/v1](hw/v1/README.md) (~€746, one drone, prove the pipeline) and [hw/v2](hw/v2/README.md) (~€13K, 10-drone production system on a truck). Two swappable payload modules on the same drone: [hw/nozzle](hw/nozzle/README.md) (paint — v1's SG90 servo pressing a standard aerosol can, ~€10) and [hw/wash](hw/wash/README.md) (pressure-wash with a 64 mm EDF counter-thrust fan that cancels the wash nozzle's reaction force, ~€50 add-on, designed for v2). Index in [hw/README.md](hw/README.md).

## Data flow

```
vanguard ──(3D scan)──▶ pantheon ──(intent)──▶ oracle ──(sorties via MAVLink)──┐
                            ▲                   │                              │
                            │                   │                  ┌───────────┼───────────┐
                            │                   │                  ▼           ▼           ▼
                            │                   │              legion 01   legion 02   legion 03
                            │                   │                 │           │           │
                            │                   │                 ▼           ▼           ▼
                            │                   │               PX4         PX4         PX4
                            │                   │                 │           │           │
                            └──(telemetry)──────┴◀────────────────┴───────────┴───────────┘
                                                    (no drone-to-drone links)
```

1. **vanguard** scans the structure.
2. **pantheon** ingests the scan; the operator authors an *intent* ("paint these regions to this spec").
3. **pantheon** hands the intent to **oracle**, which slices it into per-drone sorties (see oracle's plan/apply lifecycle).
4. After operator approval, **oracle** uploads sorties to each drone via MAVLink and supervises execution.
5. **legion** (on each drone) drives PX4 through the sortie and streams telemetry back to oracle.
6. All coordination is **star-topology through oracle** — drones never talk to each other.

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

#### pantheon — v1: build 5% / reuse 95% · long-term: build 70% / reuse 30%
Pantheon is the **CAD tool**; oracle is the **slicer**. Pantheon authors *what* should be painted (regions on a 3D mesh + paint spec + constraints) and hands it to oracle as an intent file. Oracle does everything from there: path generation, lane assignment, fleet scheduling, drone communication. The boundary is sharp on purpose — see [pantheon/README.md](pantheon/README.md) and [oracle's slicer rationale](oracle/README.md#the-core-insight-oracle-is-the-slicer).

**v1 — Blender add-on (Hivemind Plane Picker).**
Three operations and an export format. Nothing else.

```
Bridge scan (OBJ/PLY)
  → Import into Blender
    → Operator selects faces, marks them as named regions
      → Hivemind add-on exports intent.json
        → oracle ingests:  hivemind plan --intent intent.json
```

Why Blender for v1: 3D mesh viewing, rotation, and face selection are already world-class in Blender, and the `bpy` Python API makes the whole add-on ~100 lines. The custom app comes later — for now, the operator is *us*, not a truck operator, and we can use Blender. Full design and add-on code in [pantheon/README.md](pantheon/README.md).

**Long-term — custom Tauri + React app.**
Blender is not a field-ops UX. Once v1 has proven the workflow, pantheon becomes a purpose-built desktop app:
- **Tauri shell** (single binary, Win/macOS/Linux, ~10 MB, not Electron).
- **React + Three.js / React Three Fiber** for the 3D viewer. Borrow component patterns from Skybrush Live.
- **Same intent file format** — the data contract with oracle does not change, so the migration is a UI swap, not a rewrite.

What the custom app *adds* on top of v1's scope: a plan review UI (renders `HivemindPlan` from oracle with 3D path preview, timeline scrubber, approve/modify/reject), live telemetry overlay on the *same* 3D bridge model the operator authored against, and mid-execution controls wired to oracle's amendment API. **This is the core product moat.** v1 exists to learn the problem; the custom app exists to win on UX.

**v1 vs. custom pantheon at a glance:**

| Capability | v1 (Blender add-on) | Custom pantheon (later) |
|---|---|---|
| 3D scan visualization | Blender (world-class) | Three.js / R3F |
| Face selection | Blender edit mode | Built from scratch |
| Mark / name regions | Hivemind add-on | Built from scratch |
| Export intent.json | Hivemind add-on | Built from scratch |
| Plan review (3D path preview, timeline) | — (CLI summary from oracle) | Built from scratch |
| Live telemetry on 3D model | — (Skybrush Live 2D map) | Built from scratch |
| Mid-execution controls | — (oracle CLI) | Built from scratch |
| Field-ready UX | No — Blender is complex | Yes — purpose-built for operators |

#### oracle — build 30% / reuse 70%
Maps closely onto **Skybrush Server**: a Python backend that manages drone connections, distributes missions, aggregates telemetry, and handles RTK corrections.

Two viable paths:
- **Extend Skybrush Server** with paint-specific mission logic.
- **Roll a service in Python (or Rust)** on top of MAVSDK-Python or pymavswarm for full control.

The oracle↔drone link is just MAVLink over radio/WiFi — MAVSDK handles it. What Hivemind adds is the business logic: mission decomposition (turning "paint this 50m² area" into per-drone waypoint sequences), drone rotation scheduling (who breaks off to refill paint and when), and routing telemetry back to pantheon.

#### legion — build 30% / reuse 70%
**Star topology, no mesh.** Legion is the on-drone agent — one instance per drone, running on the companion computer (typically a Raspberry Pi alongside the Pixhawk). It is *not* a peer-to-peer swarm comms layer. Every drone talks only to oracle. There is no drone-to-drone communication anywhere in Hivemind.

This is deliberate, and it follows how 5,000-drone shows work: zero peer comms, all coordination through the ground station. For Hivemind, work happens within a few hundred metres of the truck — WiFi reaches every drone, latency is <50 ms, oracle can run a 5 Hz fleet monitor for everyone. Peer-to-peer mesh networking between moving drones (discovery, routing, churn handling) is an entire system that adds nothing for this use case.

What legion runs on each drone:
- **Sortie executor.** Receives sortie commands from oracle and drives PX4 through them via MAVLink offboard mode.
- **Telemetry stream.** Pushes position, attitude, battery, paint level, ToF distance, sortie progress back to oracle.
- **Local safety loop** (~10 Hz) — the last-resort layer that runs whether or not oracle is reachable: ToF wall avoidance, oracle-heartbeat watchdog (stop spraying after 5 s without contact, RTL after 30 s), battery-critical RTL, paint-empty RTL.

What legion does *not* do: collision avoidance with other drones (that's oracle's job — see [oracle's safety section](oracle/README.md#safety-and-deconfliction)), formation flying, distributed planning, or anything that requires knowing about other drones.

For v1 with one drone, legion is a few hundred lines of Python on the Pi — primarily the local safety loop and the MAVSDK-Python sortie executor. It scales unchanged to 3, then 10 drones because the architecture is star, not mesh.

### Summary table

| Module | Existing tool | Hivemind code | Notes |
|---|---|---|---|
| **vanguard** | QGroundControl + OpenDroneMap | Import pipeline | Mostly solved |
| **pantheon (v1)** | Blender + Skybrush Studio + Skybrush Live | Surface-path & refill-scheduler Blender plugins | Ship on this; learn the problem |
| **pantheon (later)** | Three.js, Skybrush Live components | 3D planning UI, mission compiler, live 3D telemetry | **Core product moat** |
| **oracle** | Skybrush Server or MAVSDK-Python | Mission decomposition, scheduling, fleet monitor (deconfliction) | Extend or build on existing |
| **legion** | PX4 offboard mode + MAVSDK-Python on Pi | On-drone sortie executor + local safety loop | Star topology, no drone-to-drone comms |
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

## Economics

Reference case throughout this section: a **medium steel bridge** with roughly **5,000 m² of paintable surface area**.

### Traditional bridge painting

The cost structure is dominated by access and labor, not paint. Scaffolding alone is typically 40–50% of the bill.

| Cost component | $/m² | % of total | Notes |
|---|---|---|---|
| Scaffolding / containment | $50–150 | 40–50% | Setup, rental, teardown, environmental containment |
| Labor (painters + riggers) | $40–100 | 30–40% | Labor is 75–80% of on-site coating cost |
| Surface prep (blasting) | $20–50 | 10–15% | Often the slowest step |
| Paint materials | $5–15 | 5–8% | Multi-coat industrial systems |
| Traffic management | $10–30 | 5–10% | Lane closures, flaggers, night work |
| **Total** | **$130–270/m²** | | |

For the 5,000 m² reference bridge: **$650K – $1.35M**. Real-world point: a Maine contract to clean and paint three underpass bridges was bid at $1,595,000.

**Timeline:** 3–6 months for a medium bridge, often longer due to weather delays and traffic restrictions.

### Hivemind drone swarm

**Capital expenditure (one-time):**

| Item | Cost | Amortized over |
|---|---|---|
| 10× custom drones (~€800 each) | €8,000 | 100+ jobs |
| Spare parts, batteries (20× sets) | €3,000 | Consumable |
| RTK base station | €2,000 | All jobs |
| Ground station (rugged tablet) | €1,000 | All jobs |
| Truck/van outfitting (refill station) | €5,000 | All jobs |
| Scout drone + camera (vanguard) | €3,000 | All jobs |
| Software development | €??? | All jobs |
| **Total hardware** | **~€22,000** | |

Amortized over 50 bridge jobs: **~€440/job**, or **~€0.09/m²**.

**Per-job operating costs:**

| Cost component | $/m² | % of total | Notes |
|---|---|---|---|
| Operators (2 people × 5 days) | $8–15 | 30–40% | Drone operator + paint tech |
| Paint materials | $5–15 | 25–35% | Same paint, same amount |
| Drone consumables (batteries, props, nozzles) | $2–5 | 10–15% | Wear items |
| Transport + setup | $2–4 | 5–10% | Truck to site, RTK setup, scan |
| Equipment amortization | $1–2 | 3–5% | Fleet depreciation |
| Insurance + regulatory | $2–5 | 5–10% | SORA, liability |
| **Total** | **$20–45/m²** | | |

For the 5,000 m² reference bridge: **$100K – $225K**. **Timeline:** 5–10 days (scan + plan + execute).

### Side-by-side

| | Traditional | Hivemind drone swarm | Delta |
|---|---|---|---|
| Cost per m² | $130–270 | $20–45 | **70–85% cheaper** |
| 5,000 m² bridge | $650K–1.35M | $100K–225K | **$500K–1.1M saved** |
| Duration | 3–6 months | 1–2 weeks | **~90% faster** |
| Workers at height | 10–20 | 0 | **Eliminated** |
| Scaffolding | Yes (40–50% of cost) | None | **Eliminated** |
| Traffic disruption | Months of lane closures | Days | **Minimal** |
| Weather sensitivity | High | Moderate | Similar |

### Where the savings actually come from

Hivemind isn't competing on painting *speed* — it's **deleting scaffolding**. Scaffolding and containment are 40–50% of a traditional bridge painting contract. They take weeks to erect, cost a fortune to rent, and create most of the traffic disruption. Removing that line item is the entire economic story. Painting throughput is secondary.

This framing matters for product priorities: any feature that lets the drones replace scaffolding (better surface following, longer endurance, refill automation) compounds. Any feature that just makes spraying marginally faster does not.

### Honest risks and caveats

These are real and shape what v1 can credibly sell:

- **Surface prep is the elephant in the room.** Traditional bridge painting is often 50%+ surface preparation — sandblasting old paint and removing rust. Drones can spray, but they cannot sandblast (the reaction forces would destabilize a small drone). v1 likely addresses **overcoating and touch-up only**, not full strip-and-repaint. That's a real subset of the market, not the whole thing.
- **Containment requirements.** Environmental rules often require capturing overspray and old-paint debris, especially over water. Traditional jobs use plastic sheeting on the scaffolding. Hivemind needs an answer: a separate netting-deployment drone, or accepting only water-based coatings, or a constrained spray pattern that minimizes overspray.
- **Coating quality verification.** Industrial coatings need specific film thickness (mils). A human painter measures wet film thickness as they go. Hivemind needs its own verification path — possibly a follow-up inspection drone with a coating thickness gauge.
- **Weather window.** Drones typically cannot fly in winds above ~30 km/h (for a 2–3 kg quad). Bridges are often windy. Traditional crews can work in moderate wind because they're on scaffolding. This narrows the operational window.

### The business case

Even confined to overcoating work, the numbers are compelling:

- The EU has ~500,000 road bridges requiring regular maintenance.
- Average maintenance painting cycle: every 10–15 years.
- 1% market capture = ~500 bridges/year.
- At ~€100K savings per bridge → **~€50M/year in value created**.

The economics clearly work. The binding constraint is not the technology — it's **SORA approval** (operational authorization for BVLOS swarm flight) and **proving coating quality meets spec**. Those, not the engineering, are what set the real timeline.
