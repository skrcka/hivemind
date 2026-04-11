# oracle

Orchestrator and integration hub for Hivemind. A mix of hardware and software that sits between **pantheon** (operator intent) and the drone swarm (execution). Oracle is the only thing in the system that talks to every drone, holds the live picture of the fleet, and decides exactly which drone does exactly what.

> See the top-level [README](../README.md) for project context, the full submodule list, and the build-vs-reuse strategy.

## Role

Oracle is the orchestrator. Concretely, it:

- Communicates with all drones over MAVLink (directly via MAVSDK-Python, or via Skybrush Server).
- Distributes RTK corrections from the truck base station to the swarm.
- Receives an **intent** from pantheon ("paint these regions on the bridge to this spec") and turns it into a concrete **plan** of per-drone sorties.
- Uploads sorties to drones, supervises execution, and routes telemetry back to pantheon.
- Handles mid-execution events that require adapting the plan: a drone goes down, wind picks up, paint runs low, the operator pauses a region.

Pantheon describes *what* should happen on the structure. Oracle decides *how* the fleet makes it happen. Drones just execute pre-validated MAVLink missions.

## The core insight: oracle is the slicer

Pantheon should not generate per-drone missions. **Oracle should.**

The reason is the same reason a CNC slicer is a separate program from the CAD tool: the operator authors *intent* (regions to paint, paint spec, constraints), but turning intent into per-machine instructions depends on facts the authoring tool doesn't have:

- How many drones are currently online and healthy?
- How much battery does each one have right now?
- How much paint capacity, and where is the refill station?
- What's the current wind forecast and the weather window?
- What no-fly zones are active?
- What's already been painted (on a replan / resume)?

These are oracle's facts. They change minute-to-minute. If pantheon baked them into a plan, the plan would be stale before it left the laptop. So pantheon hands oracle a high-level intent and oracle slices it: paths → passes → **spatial lanes** → sorties → drone assignments → schedule. The lane-assignment step is what makes static collision deconfliction work — see the [safety section](#safety-and-deconfliction) below.

This is also why oracle is the **right place to enforce safety**. It's the choke point where every drone command originates, and it has full visibility into the fleet state needed to validate "this is safe to execute right now."

## Plan / apply lifecycle

Oracle adopts the **Terraform plan/apply pattern**. The operator never gets to "drones do something the operator hasn't reviewed." Every execution is preceded by a previewable plan that the operator explicitly approves.

```
pantheon                     oracle                         drones
   │                           │                              │
   │──"paint region A+B"─────▶ │                              │
   │                           │── [PLAN phase]               │
   │                           │   generate passes            │
   │                           │   split into sorties         │
   │                           │   simulate fleet schedule    │
   │                           │   check constraints          │
   │                           │   estimate time + paint      │
   │                           │                              │
   │◀── plan proposal ────────│                              │
   │                           │                              │
   │  operator reviews:        │                              │
   │  - 3D preview of paths    │                              │
   │  - sortie count           │                              │
   │  - estimated duration     │                              │
   │  - paint consumption      │                              │
   │  - warnings/conflicts     │                              │
   │                           │                              │
   │── APPROVE / REJECT ─────▶ │                              │
   │   (or MODIFY + replan)    │                              │
   │                           │── [APPLY phase]              │
   │                           │   upload to drones ─────────▶│
   │                           │   begin execution            │
   │◀── live progress ────────│◀── telemetry ────────────────│
   │                           │                              │
   │── PAUSE / ABORT ────────▶ │── halt commands ────────────▶│
```

### Terraform parallels

| Terraform | Oracle | Purpose |
|---|---|---|
| `terraform plan` | `oracle plan` | Compute what will change, show the operator |
| Plan output diff | Plan preview in pantheon (3D viz + stats) | Human reviews before anything happens |
| `terraform apply` | `oracle apply` (operator approves) | Execute the plan |
| `-auto-approve` | **Never.** Always require human approval | Safety |
| State file | Oracle's fleet state + progress tracking | What's been done, what's pending |
| `terraform destroy` | `oracle abort` | Stop everything, land all drones |
| Drift detection | Telemetry deviation from plan | Drone isn't where it should be |
| `-target` | Approve partial plan / single region | Do region A now, region B later |
| `terraform plan` after partial apply | Replan with remaining work | Accounts for what's already painted |

The one place oracle diverges from Terraform: **mid-execution amendments**. Once apply has started, conditions change (drone fails, wind picks up, region needs to be skipped). Oracle handles this with explicit plan amendments rather than pretending state is static.

## The Plan object

A plan is a complete, inspectable description of everything that will happen — no surprises. The operator sees exactly what they're approving.

```rust
struct HivemindPlan {
    id: PlanId,
    created_at: DateTime,
    status: PlanStatus,  // Draft → Proposed → Approved → Executing → Complete

    // What the operator asked for
    intent: PaintIntent {
        regions: Vec<MeshRegion>,
        paint_spec: PaintSpec,            // type, thickness, coats
        constraints: OperatorConstraints, // time window, no-fly zones
    },

    // What oracle computed
    coverage: CoveragePlan {
        passes: Vec<SprayPass>,           // toolpath lines on the surface
        total_area_m2: f64,
        overlap_percent: f64,
        estimated_coats: u32,
    },

    sorties: Vec<Sortie>,                 // individual drone missions
    schedule: FleetSchedule {
        total_sorties: u32,
        total_duration: Duration,
        peak_concurrent_drones: u32,
        refill_cycles: u32,
    },

    resources: ResourceEstimate {
        paint_liters: f64,
        battery_cycles: u32,
        total_flight_time: Duration,
    },

    // Problems oracle found
    warnings: Vec<PlanWarning>,           // "wind forecast >25 km/h at 14:00"
    errors: Vec<PlanError>,               // "region B unreachable from truck position"

    // Diff from previous plan (if replan)
    diff: Option<PlanDiff>,
}

enum PlanStatus {
    Draft,
    Proposed,    // oracle computed, waiting for operator
    Approved,    // operator said go
    Executing,   // drones in the air
    Paused,      // operator hit pause
    Aborted,     // operator hit abort
    Complete,    // all sorties done
    Failed,      // unrecoverable error
}
```

Key properties:

- **Deterministic and previewable.** Given the same intent and the same fleet state, oracle produces the same plan. The operator can scrub through a timeline animation in pantheon showing the full ballet before approving.
- **Self-contained.** A plan is all the information needed to execute it; oracle does not need to "look things up" mid-flight.
- **Auditable.** Plans are persisted with their inputs, outputs, and approval. After-the-fact analysis (why did we use 14% more paint than estimated?) reads them back.

## Modify and replan

The operator doesn't have to accept the first plan oracle produces. They can adjust constraints and ask for a new one:

```
PROPOSE → review → "move passes on south face to morning"
   → REPLAN → review → "looks good"
   → APPROVE → EXECUTE
```

Replans are cheap. The operator can iterate on constraints (drone count, time windows, region order, simultaneous-drone caps near roads) until the plan matches their judgement of the site. Only then do drones move.

## Mid-execution amendments

Conditions during apply are not static. Oracle expects this and handles change with explicit amendments rather than silent drift:

```rust
enum PlanAmendment {
    // Oracle-initiated
    DroneDown { drone_id, remaining_sorties_reassigned_to: Vec<DroneId> },
    WeatherHold { affected_sorties, resume_estimate: DateTime },
    SortieFailed { sortie_id, reason, proposed_retry: Sortie },

    // Operator-initiated
    PauseAll,
    PauseRegion { region_id },
    SkipRegion { region_id },
    AbortAndLand,
    AddRegion { region: MeshRegion },     // triggers full replan of remaining work
}
```

Amendment policy:

- **Minor amendments** (one drone swapping for another, retrying a failed sortie) — oracle handles autonomously and notifies pantheon.
- **Major amendments** (dropping a region, weather-driven abort, adding new work) — oracle proposes, operator approves. Same plan/apply cycle, just faster because the residual is smaller.

The line between "minor" and "major" is configurable per-deployment. The default is conservative: anything that changes the *intent* requires approval; anything that just changes *which drone* fulfils a sortie does not.

## Safety and deconfliction

Collision avoidance is solved in three layers. Each layer catches what the layer above it might miss, and the boundaries between them are explicit.

### Star topology, no drone-to-drone comms

Before the layers themselves: **legion agents on the drones never talk to each other**. All coordination flows through oracle in a star.

```
                        ┌──────────┐
             ┌──────────│  oracle  │──────────┐
             │          └──────────┘          │
             ▼               ▼                ▼
        ┌─────────┐    ┌─────────┐     ┌─────────┐
        │legion 01│    │legion 02│     │legion 03│
        └─────────┘    └─────────┘     └─────────┘
             ✗ no direct links between drones ✗
```

Reasons:

- **Simplicity.** Peer-to-peer mesh networking between moving drones (discovery, routing, churn) is an entire system that adds nothing for this use case.
- **Single source of truth.** If drone 01 thinks drone 02 is at position X and oracle thinks drone 02 is at position Y, who's right? With a star topology, there is exactly one answer.
- **It scales.** 5,000-drone shows do this with zero peer comms. If it works for 5,000 it works for 10.
- **Geometry.** Bridge work happens within a few hundred metres of the truck. WiFi reaches every drone, latency is <50 ms, oracle can run a 5 Hz fleet monitor for the whole fleet.

### Layer 1 — Static deconfliction (oracle, in the slicer, before flight)

Most collision problems are solved at planning time, just like drone shows do them. When slicing the mission into sorties, oracle assigns **spatial lanes** so no two drones are ever at the same place at the same time:

```
BRIDGE FACE (top view of surface being painted)

    Lane 1          Lane 2          Lane 3
    Drone 01        Drone 02        Drone 03
    │               │               │
    ▼               ▼               ▼
    ┌──┐            ┌──┐            ┌──┐
    │  │            │  │            │  │
    │  │ ← 30 cm    │  │ ← 30 cm    │  │ ← spray width
    │  │   spray    │  │   spray    │  │
    └──┘            └──┘            └──┘
       ├─── 2 m ────┤──── 2 m ──────┤
              safe gap between drones

    Return corridor (behind the drones, away from wall):

    Drone 01 returns via lane A ────────► base
    Drone 02 returns via lane B ────────► base  (staggered by 30 s)
    Drone 03 returns via lane C ────────► base  (staggered by 60 s)
```

Oracle's slicer guarantees, at plan time:

- Minimum 3 m horizontal separation between any two simultaneous waypoints.
- Flight corridors to and from the wall do not cross.
- Return-to-base paths are staggered or use different approach lanes.

This is also where collision-validation runs in v1: Skybrush Studio's built-in checker runs over the plan inside Blender before oracle even sees it. The slicer just has to keep it satisfied.

For bridge painting this is naturally easy because drones paint **adjacent strips** on a surface — the spatial structure of the work already separates them.

### Layer 2 — Dynamic deconfliction (oracle, during flight)

Plans drift. A drone gets slowed by wind, another finishes a pass early, a refill cycle runs long. Oracle subscribes to telemetry from every legion agent and runs a real-time fleet monitor:

```python
class OracleFleetMonitor:
    MIN_SAFE_DISTANCE = 3.0  # metres

    async def monitor_fleet(self):
        while True:
            positions = self.get_all_drone_positions()

            for i, drone_a in enumerate(positions):
                for drone_b in positions[i+1:]:
                    dist = distance_3d(drone_a.pos, drone_b.pos)

                    if dist < self.MIN_SAFE_DISTANCE:
                        await self.resolve_conflict(drone_a, drone_b)
                    elif dist < self.MIN_SAFE_DISTANCE * 2:
                        await self.send_warning(drone_a, drone_b)

            await asyncio.sleep(0.2)  # 5 Hz

    async def resolve_conflict(self, drone_a, drone_b):
        # Lower-priority drone holds; higher priority = further into its sortie
        loser = drone_a if drone_a.sortie_progress < drone_b.sortie_progress else drone_b
        await self.send_to_legion(loser.id, {
            "type": "hold_position",
            "reason": "conflict_avoidance",
            "resume_when": "cleared_by_oracle",
        })
```

Resolution policy is intentionally simple: the drone closer to finishing keeps going, the other holds in place until oracle clears it. This avoids the deadlock and oscillation problems that come from anything more clever, and it's good enough because Layer 1 means conflicts are rare in the first place.

### Layer 3 — Local safety (legion agent, on each drone)

This is the **last resort** layer, and it runs entirely on the drone's companion computer with no dependency on oracle being reachable. Legion knows nothing about other drones — only about itself and what its sensors see:

```python
class LocalSafety:
    async def safety_loop(self):
        while True:
            # 1. Wall avoidance (ToF sensor)
            if self.read_tof_sensor() < 30:  # cm
                await self.emergency_pullback()

            # 2. Oracle heartbeat
            silence = self.seconds_since_oracle_contact()
            if silence > 5:
                self.pump_off()                   # stop spraying
            if silence > 30:
                await self.return_to_base()       # give up, RTL

            # 3. Battery critical
            if self.battery_percent < 15:
                self.pump_off()
                await self.return_to_base()

            # 4. Paint empty
            if self.paint_remaining_g < 20:
                self.pump_off()
                await self.return_to_base()

            await asyncio.sleep(0.1)              # 10 Hz
```

Legion's job at this layer is to keep its own drone safe and intact even if oracle dies, the WiFi link drops, or the plan turns out to be wrong about the surface. It does *not* try to keep itself away from other drones — that responsibility belongs to oracle and is handled at Layers 1 and 2.

### Layer 4 — PX4 failsafe (firmware, when everything else fails)

If oracle dies *and* legion's local safety can't handle it (e.g. companion computer crashes), PX4's built-in failsafe takes over. The trick that prevents mass-RTL collisions is to set a **different RTL altitude per drone** at provisioning time:

```
Drone 01: RTL_RETURN_ALT = 25 m
Drone 02: RTL_RETURN_ALT = 30 m
Drone 03: RTL_RETURN_ALT = 35 m
Drone 04: RTL_RETURN_ALT = 40 m
...
```

If everything fails, every drone returns home at a different altitude. They physically cannot collide. This is a standard drone-show safety pattern and it requires zero coordination — the protection is baked into static configuration on each Pixhawk.

### Who prevents what

| Collision type | Prevented by | When |
|---|---|---|
| Two drones assigned overlapping paths | Oracle slicer (Layer 1, lane assignment) | Before flight |
| Drift brings two drones too close mid-flight | Oracle fleet monitor (Layer 2) | During flight, 5 Hz |
| Drone flying into the wall | Legion local safety (Layer 3, ToF) | 10 Hz, no oracle needed |
| Oracle dies, drones must come home | Legion local safety (Layer 3, heartbeat → RTL) | After 30 s of silence |
| Legion + oracle both dead, drones RTL simultaneously | PX4 failsafe + staggered RTL altitudes (Layer 4) | Emergency only |
| Total system failure | PX4 failsafe RTL at preset altitude | Last resort |

For v1 with a single drone, **none of Layers 1 or 2 actually matter** — there are no other drones to collide with. Layers 3 and 4 still run and earn their keep (wall avoidance, oracle-link watchdog, RTL). The architecture is built to scale unchanged from 1 to 10 drones because the responsibility split is the same at every fleet size.

## What pantheon shows during review

Oracle's plan output is shaped to be reviewable in pantheon. The plan carries enough structured detail that pantheon can render this without round-tripping for more data:

```
┌─────────────────────────────────────────────────────────┐
│  PLAN #047 — North Face Region A+B          [PROPOSED]  │
├─────────────────────────────────────────────────────────┤
│                                                         │
│  ┌─────────────────────────────┐  Summary:              │
│  │                             │  ▸ 847 m² coverage     │
│  │   [3D bridge view with      │  ▸ 34 sorties          │
│  │    spray paths drawn on     │  ▸ 6 drones active     │
│  │    the surface, color-      │  ▸ ~4.2 hours          │
│  │    coded by drone]          │  ▸ 12.3 L paint        │
│  │                             │  ▸ 38 battery swaps    │
│  │         ▶ Play preview      │                        │
│  └─────────────────────────────┘  Warnings:             │
│                                   ⚠ Wind 22 km/h after  │
│  Timeline scrubber:                 15:00, 3 sorties    │
│  |████████░░░░░░░░| 4.2 hrs         may be affected     │
│                                                         │
│            [ APPROVE ]  [ MODIFY ]  [ REJECT ]          │
└─────────────────────────────────────────────────────────┘
```

## CLI for dev and testing

Oracle is a backend service and ships a CLI that mirrors the same plan/apply flow. This lets oracle be developed and tested *before* pantheon's UI is ready, and gives field engineers a no-frills fallback:

```bash
$ hivemind plan --region north-face --drones 6
Planning...

Plan #047: North Face Region A+B
  + 847.3 m² to paint
  + 34 sorties across 6 drones
  + 12.3 L paint required
  + Estimated time: 4h12m

  ⚠ WARNING: Wind forecast 22 km/h after 15:00
    3 sorties on exposed west edge may be delayed

  ✓ No conflicts detected
  ✓ All drones reachable
  ✓ Paint supply sufficient

Do you want to apply this plan? (yes/no): yes

Applying... Uploading sortie 1/34 to drone-01
            Uploading sortie 2/34 to drone-03
            ...
```

Build oracle first. Test with the CLI. Add the pretty UI in pantheon later. The CLI is not a throwaway — it stays as a debug and recovery tool for the life of the project.

## Implementation notes

- **Language:** Python (matches MAVSDK-Python and Skybrush Server) for v1, with Rust as a candidate for the v2 plan compiler if performance becomes an issue. The Plan object is defined here in Rust syntax for clarity, but v1 represents it as a typed Python dataclass / Pydantic model.
- **Foundation:** Either extend Skybrush Server with paint-specific mission logic, or roll a service on top of MAVSDK-Python (and pymavswarm). The decision is deferred until v1 prototyping; the plan/apply API surface is identical either way.
- **Persistence:** Plans, fleet state, and amendments persist to a local store on the truck. Oracle survives a restart without losing track of an in-progress job.
- **Safety choke point:** Every command to a drone passes through oracle. There is no other path. This is non-negotiable — it's what makes the plan/apply pattern actually mean something.

## Why this matters for the economics

The top-level [README's Economics section](../README.md#economics) makes the case that Hivemind's value comes from **deleting scaffolding, not painting faster**. Oracle is what makes that claim defensible:

- Scaffolding-free industrial work is only acceptable to clients and regulators if every drone movement is **previewable, approvable, and auditable**. The plan/apply lifecycle is how oracle delivers that.
- Centimetre-accurate work needs the slicer to consume *current* fleet state (battery, paint, wind, RTK status). Pre-baked plans authored in pantheon would not have this. Oracle does.
- SORA approval for BVLOS swarm flight — the binding constraint on the timeline — is much easier to argue for a system where a human explicitly approves a complete, validated plan before any drone moves than for a system where the operator points at a region and drones figure it out.

Oracle is not glue. It is the part of Hivemind that turns "we have drones and a 3D scan" into something a regulator and a customer will sign off on.
