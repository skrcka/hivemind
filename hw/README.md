# Hivemind hardware

This directory holds the hardware specification for Hivemind: ground station, drones, RTK, payload, and the wiring between them. It is the single source of truth for *what we buy*, *how it physically connects*, and *what software runs on which silicon*.

The spec is split by phase:

- **[v1/](v1/README.md)** — proof of concept. One worker drone, one laptop, no RTK, no swarm. Goal: fly to a wall, spray paint, come back. ~€746 total.
- **[v2/](v2/README.md)** — production. 10 worker drones, RTK base station, rugged ground station on the truck, refill station, scout drone. Goal: paint a real bridge commercially. ~€13K total.

Plus a payload-specific subfolder that's referenced from both phases:

- **[nozzle/](nozzle/README.md)** — spray mechanism build doc. v1's full mechanism (SG90 servo pressing a standard aerosol spray can, ~€10) lives here in detail: parts, assembly, wiring, software, test procedure, swap procedure, specs. v2 replaces this mechanism with a peristaltic pump + bayonet cartridge for industrial coatings, but the v1 rig stays as the canonical bench-test reference.

For project context (what Hivemind is, what each submodule does, the economics) see the top-level [README](../README.md). For the safety / collision-avoidance architecture that drives several hardware decisions (RTK base station, ToF sensor, distinct RTL altitudes per drone), see [oracle/README.md → Safety and deconfliction](../oracle/README.md#safety-and-deconfliction).

## System overview

```
                GROUND (Truck)                           AIR (Per Drone)

    ┌──────────────────────────────┐          ┌─────────────────────────────┐
    │  PANTHEON (UI)               │          │  LEGION AGENT               │
    │  Blender + Skybrush Studio   │          │  Python service on RPi 5    │
    │  or custom Tauri+React app   │          │                             │
    │  ─ 3D scan viewer            │          │  ─ receives sorties         │
    │  ─ region painting           │          │  ─ feeds waypoints to PX4   │
    │  ─ plan review + approve     │          │  ─ controls spray pump      │
    │  ─ live fleet status         │          │  ─ reads sensors            │
    ├──────────────────────────────┤          │  ─ reports status to oracle │
    │  ORACLE (Brain)              │          │  ─ local safety (wall avoid)│
    │  Python/Rust service         │   WiFi   │                             │
    │                              │◄────────►│                             │
    │  ─ mission slicer            │          ├─────────────────────────────┤
    │  ─ sortie generator          │          │  PX4 on PIXHAWK 6C          │
    │  ─ fleet scheduler           │          │  (flight firmware)          │
    │  ─ dynamic rebalancer        │          │                             │
    │  ─ collision deconfliction   │          │  ─ stabilization            │
    │  ─ plan/apply lifecycle      │          │  ─ GPS navigation           │
    │  ─ telemetry aggregation     │          │  ─ offboard waypoint follow │
    └──────────────────────────────┘          │  ─ failsafe RTL             │
                                              └─────────────────────────────┘
```

## Responsibility split

| Layer | Runs on | Role | Analogy |
|---|---|---|---|
| **Pantheon** | Laptop / tablet | UI — what to paint, plan review, live monitoring | CAD software |
| **Oracle** | Laptop / NUC (same machine as pantheon) | Brain — slices regions into sorties, schedules fleet, plan/apply lifecycle | 3D-print slicer (Cura) |
| **Legion agent** | Raspberry Pi on each drone | Soldier — executes one sortie at a time, controls hardware, local safety | 3D-printer firmware |
| **PX4** | Pixhawk on each drone | Autopilot — flies the drone, follows waypoints | Stepper-motor driver |

## Communication topology

```
Pantheon ←→ Oracle         : local (same machine, IPC or localhost API)
Oracle  ←→ Legion agents   : WiFi (WebSocket, JSON / protobuf)
Legion  ←→ PX4 (Pixhawk)   : UART serial (MAVLink via MAVSDK-Python)
Legion  ←→ Spray pump      : GPIO (relay or MOSFET)
Legion  ←→ Sensors         : I2C / UART (ToF distance, paint level)
RTK base → All drone GPS   : Radio broadcast (RTCM3 corrections, v2 only)
```

This is a **star topology**. Drones never talk to each other. All coordination flows through oracle. See [oracle/README.md → Star topology](../oracle/README.md#star-topology-no-drone-to-drone-comms) for the full rationale.

## Collision prevention (4 layers, hardware-relevant bits)

| Layer | Where | Hardware implication |
|---|---|---|
| Static deconfliction (oracle slicer) | Ground compute | None — pure software, but motivates the WiFi link bandwidth |
| Dynamic deconfliction (oracle fleet monitor) | Ground compute | Drives the 5 Hz telemetry budget on the WiFi link |
| Local safety (legion) | Companion Pi + ToF sensor | **TFMini-S / VL53L1X distance sensor on every drone** |
| PX4 failsafe (last resort) | Pixhawk firmware | **Different `RTL_RETURN_ALT` per drone**, set at provisioning |

Full detail in [oracle/README.md → Safety and deconfliction](../oracle/README.md#safety-and-deconfliction). The hardware items called out above (ToF sensor, per-drone RTL altitudes, RTK) exist because of those layers — don't drop them as cost savings.

## Software stack — what runs where

### Laptop / NUC (ground)

| Software | License | Role |
|---|---|---|
| **QGroundControl** | Apache 2.0 | Flight planning, telemetry display, joystick control. v1 primary interface. |
| **Skybrush Server** | GPL | Multi-drone management, RTK correction distribution, FlockWave protocol. v2. |
| **Skybrush Live** | GPL | Web-based GCS frontend. Map view, fleet status, manual override. v2. |
| **Blender + Skybrush Studio** | GPL | 3D mission planning — import bridge scan, design drone trajectories, export missions. |
| **Oracle** (our code) | ours | Mission slicer, fleet scheduler, plan/apply lifecycle, dynamic rebalancer. Python/Rust. |
| **OpenDroneMap** | AGPL | Processes scout drone photos into a georeferenced 3D mesh. |

### Raspberry Pi (each drone)

| Software | License | Role |
|---|---|---|
| **Raspberry Pi OS Lite** | free | Minimal Linux, headless. |
| **MAVSDK-Python** | BSD | Async Python API to talk MAVLink to the Pixhawk. |
| **Legion agent** (our code) | ours | Receives sorties from oracle, feeds waypoints to PX4, controls pump GPIO, reads sensors, reports status, runs the local safety loop. |

### Pixhawk (each drone)

| Software | License | Role |
|---|---|---|
| **PX4** | BSD | Flight firmware. Stabilization, GPS navigation, offboard mode, failsafe. **Do not modify.** |
