# oracle

Orchestrator and integration hub for Hivemind. The truck-side Rust binary that sits between **pantheon** (operator intent) and the drone swarm (execution). Oracle is the only thing in the system that talks to every drone, holds the live picture of the fleet, and decides exactly which drone does exactly what.

> See the top-level [README](../README.md) for project context, the full submodule list, and the build-vs-reuse strategy. The two ends of the radio link are oracle (this doc) and [legion](../legion/README.md). The wire format itself is documented in [protocol/README.md](../protocol/README.md).

## Role

Oracle is the orchestrator. Concretely, it:

- Communicates with each drone via the **`hivemind-protocol`** crate (postcard binary frames + COBS framing) over an EU-legal SiK-class telemetry radio (HolyBro 433 MHz at v1, RFD868x at v2). Oracle does **not** speak MAVLink — that's [legion](../legion/README.md)'s job, on the drone, against its own Pixhawk only.
- Distributes RTK corrections from the truck base station to the swarm as `RtkCorrection` frames (legion injects them into PX4 locally via `rust-mavlink`).
- Receives an **intent** from pantheon ("paint these regions on the bridge to this spec") and turns it into a concrete **plan** of per-drone sorties.
- Uploads each sortie to the corresponding legion in one frame, then drives a step-by-step `Proceed` handshake — every step transition is gated by oracle, and the operator can intervene at any gate.
- Handles mid-execution events that require adapting the plan: a drone goes down, wind picks up, paint runs low, the operator pauses a region.

Pantheon describes *what* should happen on the structure. Oracle decides *how* the fleet makes it happen. Legion executes the per-drone work and is the only thing that talks MAVLink to PX4.

## The core insight: oracle is the slicer

Pantheon should not generate per-drone missions. **Oracle should.**

The reason is the same reason a CNC slicer is a separate program from the CAD tool: the operator authors *intent* (regions to paint, paint spec, constraints), but turning intent into per-machine instructions depends on facts the authoring tool doesn't have:

- How many drones are currently online and healthy?
- How much battery does each one have right now?
- How much paint capacity, and where is the refill station?
- What's the current wind forecast and the weather window?
- What no-fly zones are active?
- What's already been painted (on a replan / resume)?

These are oracle's facts. They change minute-to-minute. If pantheon baked them into a plan, the plan would be stale before it left the laptop. So pantheon hands oracle a high-level intent and oracle slices it: paths → passes → **spatial lanes** → sorties → typed steps → drone assignments → schedule. The lane-assignment step is what makes static collision deconfliction work — see the [safety section](#safety-and-deconfliction) below.

This is also why oracle is the **right place to enforce safety**. It's the choke point where every drone command originates, and it has full visibility into the fleet state needed to validate "this is safe to execute right now."

## Stack at a glance

| Concern | Choice | Why |
|---|---|---|
| Language | **Rust (stable, edition 2021)** | Same as legion. Single static binary on the truck NUC, no Python runtime, lifetimes catch the kind of fleet-state races a 5 Hz monitor will otherwise hit at 3 a.m. With both binaries in Rust, the protocol crate is shared at compile time. |
| HTTP / WS server (pantheon-facing) | **axum 0.7** + tower-http | Async-first, WebSocket built in for pantheon's `/ws/telemetry` feed. **axum is *only* for the operator UI** — pantheon runs on the same NUC (or LAN), so HTTP+WS over localhost is fine. The legion link does *not* go through axum. |
| Async runtime | **tokio (multi-thread)** | Default for axum; fits the actor-per-drone model. |
| Drone link | **`hivemind-protocol`** crate (workspace member, shared with legion) | Oracle does not speak MAVLink to drones — see ["Why not MAVLink for the drone link"](#why-not-mavlink-for-the-drone-link). MAVLink lives entirely between legion and the Pixhawk on each drone, behind a UART. The truck↔drone radio carries our binary protocol. Same crate, same bytes on both ends. |
| Wire format | **postcard** + **COBS** framing, defined in `hivemind-protocol` | Postcard for compact serialization, COBS for self-synchronising frame boundaries on a serial byte stream. Full details in [protocol/README.md](../protocol/README.md#wire-format). |
| Radio transport | **Serial (`tokio-serial`)** for v1/v2 production with EU-legal SiK-class radios; **TCP (`tokio::net::TcpStream`)** for SITL, dev, and any future IP radio | Both back the same `Transport` trait from `hivemind-protocol`. The same binary runs against either; the choice is one config flag. v1 hardware is **HolyBro SiK Telemetry Radio V3 (433 MHz EU variant)**; v2 hardware is **RFDesign RFD868x** (868 MHz, EU SRD-d 500 mW sub-band, ~40 km BVLOS range). **No 915 MHz parts** — that's US/Canada-only. |
| Persistence | **SQLite via `sqlx`** with a fully relational schema | One operator on one truck = one writer. SQLite is the right shape, survives a restart per the requirement below, and `sqlx`'s compile-time-checked queries catch schema drift. The plan body stays JSON (it's the slicer's frozen output, never queried by field), but every status, FK, summary metric, step progress, and audit row is properly modelled — see [Persistence](#persistence). |
| CLI | **clap (derive)** | Same binary serves the daemon and the CLI subcommands; subcommands talk to the running daemon over a Unix socket. |
| Logging | **tracing + tracing-subscriber** | Structured logs, spans across async boundaries, JSON output for shipping off the truck post-job. |
| Geometry | **glam** (or **nalgebra** if we need SVD for Kabsch) | Lightweight vector math for lane assignment and the pre-flight alignment offset. |
| Errors | **thiserror** in libraries, **anyhow** at the binary edges | Standard split. |
| Time / IDs | **time** + **uuid v7** | Sortable plan IDs, no `chrono` cve churn. |
| Config | **figment** (TOML + env overlays) | One config file on the truck, env vars for ops to override at startup. |
| Tests | **`cargo test`**, **`insta`** for plan-snapshot tests, a Rust **legion mock** in `tests/`, **real legion + PX4 SITL** as a nightly gate | Plans are deterministic given the same intent + fleet state — snapshot tests catch slicer regressions. The mock links the same `hivemind-protocol` crate so wire-format drift is impossible. |

Everything else (refill logistics, multi-truck, SORA paperwork) is out of scope for v1.

## Workspace structure

Oracle is one of four Rust members of a top-level Cargo workspace:

```
hivemind/
├── Cargo.toml                   ← workspace root
├── README.md
│
├── protocol/                    ← shared wire types: hivemind-protocol  (no_std + alloc)
│   ├── README.md                ← canonical reference for the wire format
│   ├── Cargo.toml
│   └── src/
│
├── legion-core/                 ← portable executor + safety + hardware traits  (no_std + alloc)
│   ├── Cargo.toml
│   └── src/
│
├── oracle/                      ← THIS CRATE — truck-side runtime  (std + tokio + axum + sqlx)
│   ├── README.md                ← this file
│   ├── Cargo.toml               ← depends on hivemind-protocol  (does NOT depend on legion-core)
│   └── src/
│
├── legion/                      ← Pi-side runtime  (std + tokio + rppal + rust-mavlink)
│   ├── README.md
│   ├── Cargo.toml               ← depends on legion-core + hivemind-protocol
│   └── src/
│
├── (future) legion-mcu/         ← MCU-side runtime  (no_std + embassy + embedded-hal)
│   └── ...                      ← v3 deliverable; depends on legion-core + hivemind-protocol
│
└── pantheon/, vanguard/, hw/    ← not in the workspace (Blender add-on, docs, hardware)
```

The shared crates are the contract: `hivemind-protocol` defines what goes on the wire, `legion-core` defines what the drone-side state machine *does*. Oracle imports `hivemind-protocol` only — wire-format drift between oracle and legion is a `cargo check` error.

Note that **oracle does *not* depend on `legion-core`**. The MCU-portability split is a legion concern; oracle is a NUC binary and never needs to compile no_std. Oracle treats whatever's on the other end of the radio as "speaks the protocol" and doesn't care whether it's `legion` (Pi, today) or `legion-mcu` (Cortex-M, future).

## Why not MAVLink for the drone link

MAVLink is built for **flight controller ↔ ground station** communication. The oracle ↔ legion link is doing something different: high-level swarm coordination, sortie uploads, step gating, payload state, plan/apply auditing. Those are application-level concerns that MAVLink wasn't designed for.

| | MAVLink-direct from oracle | Custom protocol via legion |
|---|---|---|
| Custom data (sortie shape, paint, ToF, step gating) | Tunnelled through `MAV_CMD_USER_*` and a custom dialect — works but ugly | First-class fields in our own types |
| Step-confirmation handshake | No native concept — abuse mission-progress messages | Native to the protocol |
| Acknowledgment richness | Basic ACK/NACK | "Step 3 complete, paint at 320 ml, battery 64%, took 47 s" |
| Sortie upload | `MISSION_ITEM_INT` upload, item-by-item handshake — slow over a 57600-baud radio | One frame, validated, persisted on receipt |
| PX4 firmware coupling | Oracle inherits PX4 mission-item semantics, mode names, parameter IDs | None — legion absorbs all of this |
| Operator audit trail | Hard | Trivial — every frame logged with structured fields |

MAVLink stays where it belongs: between legion and the Pixhawk (UART, on the same drone, never touches the radio). Read the matching argument from legion's side in [legion/README.md → Why not MAVLink for the oracle ↔ legion link](../legion/README.md#why-not-mavlink-for-the-oracle--legion-link).

## Process model

Oracle runs as **one binary, one OS process**, with internal concurrency built on tokio tasks. There is no microservice split. The truck has one job; the operator has one pane of glass; oracle is one process with a tight responsibility split inside.

```
                                ┌──────────────────────────────────┐
   pantheon (Blender) ──────────│  axum HTTP+WS API task           │
   curl / hivemind CLI ─────────│  REST + /ws/telemetry            │
                                │  (operator-facing only — no WS   │
                                │   for the legion link)           │
                                └────────────┬─────────────────────┘
                                             │
                                             ▼
                                ┌──────────────────────────────────┐
                                │           AppState (Arc)         │
                                │  ┌──────────┐  ┌──────────────┐  │
                                │  │ Store    │  │ Fleet (RwLk) │  │
                                │  │ (SQLite) │  │  per-drone   │  │
                                │  └──────────┘  └──────────────┘  │
                                │  ┌──────────────────────────┐    │
                                │  │ broadcast<TelemetryEvt>  │    │
                                │  └──────────────────────────┘    │
                                └─────┬──────┬───────────┬─────────┘
                                      │      │           │
                ┌─────────────────────┘      │           └────────────────────┐
                │                            │                                │
                ▼                            ▼                                ▼
   ┌──────────────────────┐    ┌──────────────────────┐         ┌─────────────────────┐
   │ Plan Engine (slicer) │    │ Apply Supervisor     │         │ Fleet Monitor       │
   │  - on demand, runs   │    │  - one task per      │         │  - 5 Hz tick        │
   │    on tokio blocking │    │    Approved plan     │         │  - reads positions, │
   │    pool for CPU work │    │  - drives the step-  │         │    runs Layer 2     │
   │  - pure function:    │    │    confirmation      │         │    deconfliction    │
   │    Intent + Fleet →  │    │    handshake         │         │  - emits Hold cmds  │
   │    HivemindPlan      │    └──────────┬───────────┘         └──────────┬──────────┘
   └──────────────────────┘               │                                │
                                          │                                │
                                          ▼                                ▼
                              ┌────────────────────────────────────────────────────┐
                              │            Legion Link (one actor)                 │
                              │  - the *only* path to a drone                      │
                              │  - per-drone Transport (serial UART or TCP)        │
                              │  - per-drone command mailbox (serialised)          │
                              │  - decodes telemetry → broadcast + FleetState      │
                              │  - enforces "command must come from an Approved    │
                              │    plan or a privileged abort/RTL path"            │
                              └────────────────────┬───────────────────────────────┘
                                                   │ postcard + COBS frames
                                                   │ (hivemind-protocol crate)
                                                   ▼ over the radio link
                              ┌────────────────────────────────────────────────────┐
                              │  legion 01 .. legion N                             │
                              │  (Rust on the Pi; rust-mavlink → Pixhawk via       │
                              │   TELEM2 UART — MAVLink lives here, not in oracle) │
                              └────────────────────────────────────────────────────┘
```

Three things to notice:

1. **The Legion Link is the safety choke point.** There is no other Rust module allowed to open a legion connection or send a command frame. This is enforced by module privacy: the `legion_link` submodule exposes only a `Link` handle, and `Link::send_command` requires a `CommandAuthority` token that only the Apply Supervisor (with an Approved plan) and the explicit safety/abort path can mint.
2. **The fleet monitor is its own task, not a method call inside the apply loop.** It ticks at 5 Hz no matter what apply is doing, because Layer 2 deconfliction has to keep running while a sortie executor is blocked waiting on a slow drone.
3. **The plan engine is a pure function**, not a long-lived task. It takes `(Intent, FleetSnapshot, Constraints)` and returns `HivemindPlan`. Determinism is the cheapest way to make plans previewable and snapshot-testable. It runs on `tokio::task::spawn_blocking` because lane-packing and area subdivision are CPU-bound.

There is also an **independent backup channel** that lives outside oracle entirely: an optional second SiK radio between the truck (running QGroundControl) and each drone's Pixhawk TELEM1. This gives the operator a parallel monitoring + manual-override path that doesn't depend on oracle, legion, or our protocol being healthy. Plumbing it is a hardware decision tracked in [hw/](../hw/README.md), not a backend concern.

## Module layout

```
oracle/
├── Cargo.toml                     ← workspace member
├── README.md                      ← this file
├── migrations/                    ← sqlx migrations
│   └── 0001_init.sql              ← the schema in §Persistence
├── src/
│   ├── main.rs                    ← clap entrypoint, dispatches to serve / plan / apply / status
│   ├── lib.rs                     ← re-exports for integration tests
│   │
│   ├── config.rs                  ← figment-loaded config (paths, ports, legion-link settings)
│   ├── error.rs                   ← thiserror enum + axum IntoResponse impl
│   │
│   ├── domain/                    ← oracle-side types (the slicer's input/output, not wire types)
│   │   ├── mod.rs
│   │   ├── intent.rs              ← Intent, MeshRegion, PaintSpec — mirrors pantheon's intent.json
│   │   ├── plan.rs                ← HivemindPlan, PlanStatus, PlanDiff
│   │   ├── fleet.rs               ← Drone, DroneState, FleetSnapshot
│   │   └── amendment.rs           ← PlanAmendment enum
│   │   //  Note: Sortie / SortieStep / RadioLossPolicy are NOT defined here.
│   │   //  They live in the `hivemind-protocol` crate (the wire types).
│   │
│   ├── api/                       ← axum routers (pantheon-facing only)
│   │   ├── mod.rs                 ← Router::new(), middleware stack
│   │   ├── intents.rs             ← POST /intents, GET /intents/:id
│   │   ├── plans.rs               ← POST /plans, GET /plans/:id, POST /plans/:id/approve, /abort, /amendments, /sorties/.../proceed
│   │   ├── fleet.rs               ← GET /fleet, GET /fleet/:drone_id
│   │   └── ws.rs                  ← /ws/telemetry (pantheon subscribers, not legion)
│   │
│   ├── slicer/                    ← the plan engine (pure)
│   │   ├── mod.rs                 ← entrypoint: plan(intent, fleet, cfg) -> Result<HivemindPlan>
│   │   ├── coverage.rs            ← regions → spray passes (toolpath lines on the surface)
│   │   ├── lanes.rs               ← passes → spatial lanes (Layer 1 static deconfliction)
│   │   ├── sortie_pack.rs         ← lanes → sorties; each sortie is a list of typed Steps
│   │   ├── steps.rs               ← Step assembly: Takeoff → Transit → SprayPass(es) → RTL → Land
│   │   ├── radio_loss.rs          ← per-step radio-loss policy assignment (defaults + overrides)
│   │   ├── schedule.rs            ← sorties → fleet schedule, with refill cycles
│   │   ├── resources.rs           ← paint volume + battery cycle estimation
│   │   └── validate.rs            ← warnings + errors collected on the plan
│   │
│   ├── apply/                     ← apply phase
│   │   ├── mod.rs
│   │   ├── supervisor.rs          ← per-plan task: drives sortie dispatch + step handshake
│   │   ├── handshake.rs           ← the StepComplete → validate → Proceed/Hold/Abort loop
│   │   ├── gate.rs                ← step-gate decisions: AutoProceed | OperatorRequired | FleetConflict
│   │   └── amendments.rs          ← applies a PlanAmendment to an executing plan
│   │
│   ├── fleet/                     ← live fleet state + Layer 2 monitor
│   │   ├── mod.rs                 ← FleetState struct + Arc<RwLock<…>>
│   │   ├── snapshot.rs            ← copy-out helper used by the slicer
│   │   └── monitor.rs             ← 5 Hz dynamic deconfliction tick
│   │
│   ├── legion_link/               ← the only code that opens a Transport to a drone
│   │   ├── mod.rs                 ← Link handle + CommandAuthority token
│   │   ├── server.rs              ← TCP listener (or serial port opener) — legion connects in
│   │   ├── session.rs             ← per-drone session: heartbeat, reconnect, mailbox
│   │   └── transport.rs           ← thin wrapper around hivemind_protocol::Transport
│   │
│   ├── store/                     ← SQLite persistence (one module per table family)
│   │   ├── mod.rs                 ← Store handle, transaction helpers
│   │   ├── intents.rs
│   │   ├── plans.rs
│   │   ├── sorties.rs
│   │   ├── steps.rs               ← per-step progress + restart-resume
│   │   ├── drones.rs              ← fleet roster
│   │   ├── amendments.rs
│   │   └── audit.rs               ← append-only audit log of every command and approval
│   │
│   └── cli/                       ← clap subcommands; thin clients over the daemon's HTTP socket
│       ├── mod.rs
│       ├── serve.rs               ← `oracle serve`
│       ├── plan.rs                ← `oracle plan --intent intent.json --drones 6`
│       ├── apply.rs               ← `oracle apply <plan-id>`
│       ├── status.rs              ← `oracle status`
│       └── abort.rs               ← `oracle abort`
│
└── tests/
    ├── slicer_snapshots.rs        ← insta snapshot tests of HivemindPlan against fixture intents
    ├── api_smoke.rs               ← spin up the axum app, POST an intent, walk plan/approve/abort
    ├── handshake.rs               ← apply supervisor against a fake legion that scripts step-complete sequences
    └── sitl_pair.rs               ← optional CI gate: real legion + PX4 SITL on the other side
```

## Plan / apply lifecycle

Oracle adopts the **Terraform plan/apply pattern**. The operator never gets to "drones do something the operator hasn't reviewed." Every execution is preceded by a previewable plan that the operator explicitly approves.

```
pantheon                     oracle                         drones
   │                           │                              │
   │──"paint region A+B"─────▶ │                              │
   │                           │── [PLAN phase]               │
   │                           │   generate passes            │
   │                           │   split into typed steps     │
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
   │                           │   upload sortie ────────────▶│
   │                           │   step-by-step Proceed       │
   │                           │   handshake with each drone  │
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
| State file | Oracle's fleet state + step progress in SQLite | What's been done, what's pending |
| `terraform destroy` | `oracle abort` | Stop everything, RTL all drones |
| Drift detection | Telemetry deviation from plan | Drone isn't where it should be |
| `-target` | Approve partial plan / single region | Do region A now, region B later |
| `terraform plan` after partial apply | Replan with remaining work | Accounts for what's already painted |

The one place oracle diverges from Terraform: **mid-execution amendments**. Once apply has started, conditions change (drone fails, wind picks up, region needs to be skipped). Oracle handles this with explicit plan amendments rather than pretending state is static.

## Domain model

### The Plan

```rust
// src/domain/plan.rs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HivemindPlan {
    pub id: PlanId,                       // uuid v7, sortable
    pub created_at: OffsetDateTime,
    pub status: PlanStatus,

    // What the operator asked for
    pub intent: Intent,

    // What oracle computed
    pub coverage: CoveragePlan,           // total area, overlap %, estimated coats
    pub sorties: Vec<Sortie>,             // each sortie is a list of typed Steps with policies
    pub schedule: FleetSchedule,          // total duration, peak concurrent drones, refill cycles
    pub resources: ResourceEstimate,      // paint ml, battery cycles, total flight time

    // Problems oracle found
    pub warnings: Vec<PlanWarning>,
    pub errors: Vec<PlanError>,

    // Diff from previous plan (if replan)
    pub diff: Option<PlanDiff>,

    // The slicer's input snapshot, frozen — makes the plan fully self-contained
    pub fleet_snapshot: FleetSnapshot,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum PlanStatus {
    Draft, Proposed, Approved, Executing, Paused, Aborted, Complete, Failed,
}
```

Key properties:

- **Deterministic and previewable.** Given the same intent and the same fleet state, the slicer produces the same plan. The operator can scrub through a timeline animation in pantheon showing the full ballet before approving.
- **Self-contained.** A plan is all the information needed to execute it; oracle does not need to "look things up" mid-flight. `fleet_snapshot` is inlined for exactly this reason.
- **Auditable.** Plans persist with their inputs, outputs, and approval. After-the-fact analysis (why did we use 14% more paint than estimated?) reads them back.

### Sortie / SortieStep / RadioLossPolicy

These types live in **`hivemind-protocol`** because both oracle and legion need the same definitions on the wire. See [protocol/README.md → Sortie / SortieStep / RadioLossPolicy](../protocol/README.md#sortie--sortiestep--radiolosspolicy) for the canonical definitions. Oracle imports them as:

```rust
use hivemind_protocol::{Sortie, SortieStep, StepType, RadioLossPolicy, RadioLossBehaviour, Waypoint};
```

The design is non-negotiable: oracle's slicer produces these exact types, oracle's apply supervisor uploads them as exact wire frames, legion deserializes them on the other side. There is no oracle-side `Sortie` type and a separate legion-side `Sortie` type — that's the whole point of having a shared crate.

## The slicer, concretely

Plan generation runs as:

```rust
pub fn plan(
    intent: Intent,
    fleet: FleetSnapshot,
    cfg: &SlicerConfig,
) -> Result<HivemindPlan, SlicerError> {
    let coverage    = coverage::generate_passes(&intent, cfg)?;        // surface → toolpath lines
    let lanes       = lanes::assign(&coverage, &fleet, cfg)?;          // Layer 1 static deconfliction
    let raw_sorties = sortie_pack::pack(&lanes, &fleet, cfg)?;         // honour battery + paint capacity
    let sorties     = steps::assemble(raw_sorties, cfg)?;              // wrap each sortie in typed Steps
    let sorties     = radio_loss::assign_defaults(sorties, &intent.constraints, cfg);
    let schedule    = schedule::build(&sorties, &fleet, cfg)?;         // wall-clock + refill cycles
    let resources   = resources::estimate(&coverage, &sorties, cfg);
    let (warnings, errors) = validate::run(&intent, &fleet, &schedule, cfg);

    Ok(HivemindPlan { /* … */ })
}
```

The non-obvious bits:

- **`coverage::generate_passes`** turns each `MeshRegion` into a set of straight-line spray passes parallel to one principal axis of the region's bounding rectangle, spaced by the configured spray width minus overlap. v1 only handles flat or near-flat regions (the dot product of all face normals against the region centroid normal must agree within a threshold) — anything else returns a `PlanError::NonPlanarRegion`.
- **`lanes::assign`** packs passes into spatial lanes such that no two simultaneous passes are within `MIN_HORIZONTAL_SEPARATION` of each other. For bridge-style adjacent-strip work this is naturally easy; the slicer just walks passes in order and assigns each to the lowest-numbered lane that's free at that timestep. This is the parent README's Layer 1.
- **`sortie_pack::pack`** is the equivalent of a CNC slicer's tool-change planning. Each drone has finite paint and battery; a sortie is the work one drone can do between refills.
- **`steps::assemble`** wraps each packed sortie in the typed step sequence legion expects: `Takeoff → Transit(to first pass) → SprayPass(...) → Transit → SprayPass(...) → RefillApproach → RefillWait → ... → ReturnToBase → Land`. This is the slicer's responsibility because it has the full context (paint state, refill schedule, runway position).
- **`radio_loss::assign_defaults`** stamps a `RadioLossPolicy` onto every step. Defaults come from a per-StepType policy table (see [legion's defaults](../legion/README.md#the-sortie)) but are overridden by `intent.constraints` for cases like "any spray pass over an active road must `RtlImmediately` on radio loss." The result is per-step.
- **`schedule::build`** lays sorties on a wall-clock timeline, interleaving refill cycles, and computes the `peak_concurrent_drones` and `total_duration` fields. This is what pantheon's timeline scrubber renders.
- **`validate::run`** never fails the plan; it produces warnings (`Wind forecast >25 km/h at 14:00`) and errors (`Region B unreachable from truck position`). A plan with errors is still a Plan — it just can't be `Approved`. The operator sees the errors in the proposal.

For v1 with one drone, lane assignment is a no-op, sortie packing is one sortie per battery, and the schedule is linear. The architecture exists at v1 because changing it later for 10 drones would be a rewrite.

## Apply phase — the step-confirmation handshake

This is the heart of the radio-first design: every step transition is gated explicitly. Oracle uploads the full sortie up-front, then drives a `StepComplete → validate → Proceed/Hold/Abort` loop for the lifetime of the sortie. The drone always has the full plan locally — the handshake is the *coordination layer*, not the data layer.

When the operator POSTs `/plans/{id}/approve`:

1. The HTTP handler transitions the plan row from `Proposed` → `Approved` in a SQLite transaction, writes an audit-log entry capturing operator identity and the plan hash, and spawns a new **Apply Supervisor** task seeded with the plan id.
2. The Apply Supervisor reads the plan and builds an in-memory `SortieQueue`. For each sortie, it runs the handshake loop:

```rust
async fn supervise_sortie(
    sortie: &Sortie,
    plan_id: PlanId,
    link: &Link,
    fleet: &FleetState,
    auth: CommandAuthority,
) -> Result<(), ApplyError> {
    // 1. Upload the full sortie. Legion validates, persists, and ACKs.
    link.upload_sortie(sortie.drone_id, sortie, &auth).await?;
    wait_for(|m| m.is_sortie_received(&sortie.sortie_id)).await?;

    // 2. Walk the steps with explicit gating.
    for step in &sortie.steps {
        match gate::evaluate(plan_id, sortie, step, fleet) {
            Gate::AutoProceed => {
                link.send_proceed(sortie.drone_id, &sortie.sortie_id, step.index, &auth).await?;
            }
            Gate::OperatorRequired { reason } => {
                wait_for_operator_proceed(plan_id, &sortie.sortie_id, step.index, reason).await?;
                link.send_proceed(sortie.drone_id, &sortie.sortie_id, step.index, &auth).await?;
            }
            Gate::FleetConflict { with } => {
                link.send_hold(sortie.drone_id, &sortie.sortie_id, format!("conflict with {with}"), &auth).await?;
                wait_for_fleet_clearance(&with).await?;
                link.send_proceed(sortie.drone_id, &sortie.sortie_id, step.index, &auth).await?;
            }
            Gate::AbortSortie { reason } => {
                link.send_abort(sortie.drone_id, &sortie.sortie_id, reason, AuthorityKind::Plan(auth)).await?;
                return Err(ApplyError::Aborted);
            }
        }

        let completion = wait_for_step_complete(&sortie.sortie_id, step.index).await?;
        store.record_step(plan_id, &sortie.sortie_id, step.index, &completion).await?;
    }

    wait_for(|m| m.is_sortie_complete(&sortie.sortie_id)).await?;
    Ok(())
}
```

The `gate::evaluate` step is where oracle's value lives. Per step it checks:

- **Fleet deconfliction.** Is any other drone within the safety envelope of this step's path? If so, `FleetConflict { with }`.
- **Resource budget.** Does this drone still have enough paint and battery for the step? If not, `AbortSortie { reason: "paint depleted" }`.
- **Operator-required gate.** Did the slicer mark this step as requiring human approval (e.g. the first spray pass of the day, or any step within a no-fly buffer)? If so, `OperatorRequired { reason }`.
- **Weather window.** Is the wind forecast for the next `expected_duration` still within limits?
- **Plan-level pause.** Did the operator hit pause on the plan? If so, all sortie supervisors hold their drones at the next gate.

If everything passes, `AutoProceed`. For v1 with one drone and a clear weather forecast, almost every gate evaluates to `AutoProceed` instantly — but the plumbing exists from day one because every other multiplier in the system depends on it.

3. The supervisor persists per-step progress to SQLite on every transition. A crash + restart resumes from the last persisted step using the `step_progress` table — see [Persistence](#persistence).
4. **Mid-execution amendments** arrive via `POST /plans/{id}/amendments`. The supervisor classifies them as minor (autonomous) or major (operator-approval-required) per the policy below.

There is exactly one Apply Supervisor task per `Executing` plan. Trying to approve a second plan while one is still executing returns `409 Conflict` — oracle does not multi-tenant a fleet.

### What the operator sees

For every step that hits `OperatorRequired` (or any step that legion is currently held on), pantheon sees a `step_awaiting_proceed` event on the telemetry stream and surfaces a "proceed / hold / abort" affordance in its sortie review panel. The operator's click maps to one of:

- `POST /v1/plans/:id/sorties/:sortie/steps/:idx/proceed`
- `POST /v1/plans/:id/sorties/:sortie/steps/:idx/hold`
- `POST /v1/plans/:id/sorties/:sortie/abort`

The supervisor's `wait_for_operator_proceed` future is signalled by the matching POST. The audit log records who approved each step.

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

## Fleet Monitor (Layer 2 deconfliction)

A separate task running independently of any Apply Supervisor:

```rust
pub async fn run(state: Arc<AppState>) {
    let mut tick = tokio::time::interval(Duration::from_millis(200)); // 5 Hz
    loop {
        tick.tick().await;

        let snapshot = state.fleet.read().snapshot_positions();
        let conflicts = detect_conflicts(&snapshot, MIN_SAFE_DISTANCE_M);

        for c in conflicts {
            // Lower-priority drone holds; higher = further into its sortie.
            let loser = c.lower_priority();
            state.link.hold_position(loser, HoldReason::Conflict, AuthorityKind::SafetyOverride).await;
            state.audit.record(AuditEvent::ConflictHold { drones: c.pair(), distance: c.distance });
        }
    }
}
```

Notes:

- The monitor uses an `AuthorityKind::SafetyOverride` token, mintable without an Approved plan because hold/abort commands must always be permitted. Mint is recorded to the audit log.
- Resolution is intentionally simple: nearer-to-finishing wins, the other holds, oracle clears it later. No clever escape geometry, no oscillation.
- The monitor reads positions from `FleetState`, fed by the Legion Link's per-drone telemetry decoder. There's no separate position source — one path, one truth.
- For v1 with one drone, this task starts but does nothing — `detect_conflicts` returns empty. It's wired in at v1 so the architecture is the same at 1, 3, and 10 drones.

## Safety and deconfliction

Collision avoidance is solved in **four layers**. Each layer catches what the layer above it might miss, and the boundaries between them are explicit.

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
- **Geometry.** Bridge work happens within radio range of the truck. The radio reaches every drone, oracle can run a 5 Hz fleet monitor for the whole fleet.

### Layer 1 — Static deconfliction (oracle slicer, before flight)

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

    Return corridors are staggered or use different lanes entirely.
```

Oracle's slicer guarantees, at plan time:

- Minimum 3 m horizontal separation between any two simultaneous waypoints.
- Flight corridors to and from the wall do not cross.
- Return-to-base paths are staggered or use different approach lanes.

For bridge painting this is naturally easy because drones paint **adjacent strips** on a surface — the spatial structure of the work already separates them.

### Layer 2 — Dynamic deconfliction (oracle fleet monitor, during flight)

See [Fleet Monitor](#fleet-monitor-layer-2-deconfliction) above. 5 Hz tick, distance-based conflict detection, simple "loser holds" resolution policy. Only matters at v2+ multi-drone scale; v1 is a no-op.

### Layer 3 — Local safety (legion, on each drone)

This is the **last resort** layer and runs entirely on the drone's companion computer with no dependency on oracle being reachable. Legion knows nothing about other drones — only about itself and what its sensors see:

- Wall avoidance (ToF sensor reading + emergency pullback)
- Battery critical (pump off, RTL)
- Paint empty (pump off, RTL)
- Oracle silent (pump off; per-step radio-loss policy decides flight outcome)

Full details in [legion/README.md → The local safety loop](../legion/README.md#the-local-safety-loop). Legion's job at this layer is to keep its own drone safe and intact even if oracle dies, the radio link drops, or the plan turns out to be wrong about the surface.

### Layer 4 — PX4 failsafe (firmware, when everything else fails)

If oracle dies *and* legion's local safety can't handle it (e.g. companion computer crashes), PX4's built-in failsafe takes over. The trick that prevents mass-RTL collisions is to set a **different RTL altitude per drone** at provisioning time:

```
Drone 01: RTL_RETURN_ALT = 25 m
Drone 02: RTL_RETURN_ALT = 30 m
Drone 03: RTL_RETURN_ALT = 35 m
Drone 04: RTL_RETURN_ALT = 40 m
...
```

If everything fails, every drone returns home at a different altitude. They physically cannot collide. Standard drone-show safety pattern, zero coordination required, baked into static configuration on each Pixhawk.

### Who prevents what

| Collision type | Prevented by | When |
|---|---|---|
| Two drones assigned overlapping paths | Oracle slicer (Layer 1, lane assignment) | Before flight |
| Drift brings two drones too close mid-flight | Oracle fleet monitor (Layer 2) | During flight, 5 Hz |
| Drone flying into the wall | Legion local safety (Layer 3, ToF) | 10 Hz, no oracle needed |
| Oracle dies, drones must come home | Per-step radio-loss policy in legion (Layer 3) | At each step's silent timeout |
| Legion + oracle both dead, drones RTL simultaneously | PX4 failsafe + staggered RTL altitudes (Layer 4) | Emergency only |
| Total system failure | PX4 failsafe RTL at preset altitude | Last resort |

For v1 with a single drone, **none of Layers 1 or 2 actually matter** — there are no other drones to collide with. Layers 3 and 4 still run and earn their keep. The architecture is built to scale unchanged from 1 to 10 drones because the responsibility split is the same at every fleet size.

## Legion Link

The bridge to the drones is a long-lived **postcard-over-COBS** session per drone, between oracle (truck NUC) and [legion](../legion/README.md) (Pi on each drone). Both ends are Rust binaries linking the same `hivemind-protocol` crate; the wire format is binary, transport-agnostic, and carried directly on the radio link.

### Connection model

The Legion Link sits behind a `hivemind_protocol::Transport` trait. Two concrete impls:

- **`SerialTransport`** (v1/v2 production). Oracle opens `/dev/ttyUSB0` (the truck-side SiK base) at 57600 baud and treats it as one persistent session keyed by the `drone_id` from legion's `Hello` frame. There is no "connect" — the port is always open. The actor reads bytes, runs them through a COBS decoder, dispatches complete frames to postcard, and produces `LegionToOracle` messages.
- **`TcpTransport`** (SITL, dev, future IP radios). Oracle's `legion_link::server::accept_loop()` runs a `TcpListener` on a configured port; legion (or a test mock) connects in. Each accepted socket becomes a `Session`. The same COBS-postcard codec runs over the TCP byte stream.

A few things to notice:

- **Same bytes either way.** The choice of transport is purely about the byte source/sink; the framing and codec are identical. A test that runs against `TcpTransport` exercises the exact same protocol code path as production.
- **Multi-drone over a single serial line.** SiK radios broadcast — every air-side radio hears every ground-side frame. Oracle keys sessions by the `drone_id` carried in legion's `Hello` and per-frame in subsequent messages, then routes outbound commands to the right drone by including the same id in the command envelope. v1 only has one drone, so this just works; the v2 multi-drone path uses the same mechanism.
- **Authentication.** v1 carries a shared bearer token in the `Hello` exchange. Truck-local trust domain, one operator. v2 upgrades to mTLS over `TcpTransport`, or a HMAC-of-shared-key proof-of-knowledge in the `Hello` for `SerialTransport` where TLS isn't free.

### Public surface

The only way to talk to a drone from anywhere else in oracle:

```rust
pub struct Link { /* opaque */ }

impl Link {
    pub async fn upload_sortie(&self, drone: DroneId, sortie: &Sortie, auth: &CommandAuthority) -> Result<(), LinkError>;

    /// Tell legion it can start the next step. `expected_step_index` must match
    /// what legion is currently waiting on, otherwise legion rejects with an Error.
    pub async fn send_proceed(&self, drone: DroneId, sortie: &SortieId, expected_step_index: u32, auth: &CommandAuthority) -> Result<(), LinkError>;

    pub async fn send_hold(&self, drone: DroneId, sortie: &SortieId, reason: String, auth: &CommandAuthority) -> Result<(), LinkError>;
    pub async fn send_abort(&self, drone: DroneId, sortie: &SortieId, reason: String, auth: AuthorityKind) -> Result<(), LinkError>;
    pub async fn return_to_base(&self, drone: DroneId, reason: String, auth: AuthorityKind) -> Result<(), LinkError>;
    pub async fn hold_position(&self, drone: DroneId, reason: HoldReason, auth: AuthorityKind) -> Result<(), LinkError>;
    pub async fn send_rtk(&self, drone: DroneId, payload: Bytes, auth: AuthorityKind) -> Result<(), LinkError>;
    pub async fn broadcast_rtk(&self, payload: Bytes) -> Result<(), LinkError>;

    pub fn telemetry(&self) -> broadcast::Receiver<TelemetryEvent>;
}
```

Note what's missing: there's no `arm()`, no `takeoff()`, no `set_offboard_mode()`. Those are legion's job. Oracle deals in sortie + step semantics; legion translates into MAVLink calls.

### Wire protocol

The full message catalogue is documented in [protocol/README.md → Message catalogue](../protocol/README.md#message-catalogue). Both ends import the same Rust types from the `hivemind-protocol` crate; postcard handles serialization, COBS handles framing. There is no JSON on the legion link, no WebSocket, no separate "Python mirror" — it's one crate, one set of types, two binary consumers.

A typical `Telemetry` frame is ~80 bytes on the wire; a typical `UploadSortie` is ~2–5 kB depending on step count. Both fit comfortably in the SiK budget.

Heartbeat policy:

- **Oracle → legion: 2 Hz.** Legion's safety loop watches the gap and triggers `oracle_silent` after 5 s of nothing. The behaviour after that is governed by the *current step's* radio loss policy.
- **Legion → oracle: 2 Hz**, piggy-backed on the `Telemetry` message stream. Oracle marks the drone `Stale` in `FleetState` if it misses N in a row.
- **Connection drop ≠ drone failure.** Legion keeps executing whatever the current step's policy says to do. The Apply Supervisor sees a brief `Stale` flap but the in-flight sortie is not auto-aborted — legion has the full sortie locally.

### Per-session internals

- One `tokio::task` reading bytes from the `Transport`, decoding COBS-postcard frames, and dispatching to: the broadcast channel (for `Telemetry`), the in-memory `FleetState` (for state updates), and the audit log (for `SafetyEvent` and `Error`).
- One `mpsc<OracleToLegion>` mailbox driving writes. The actor serialises sends per drone — no two tasks can race on the same Transport.
- Backpressure: if a slow legion can't drain the mailbox at the configured rate, the session drops the oldest *non-safety* command and logs an audit warning. Safety commands (`HoldStep`, `AbortSortie`, `ReturnToBase`) are never dropped.
- A heartbeat watchdog: if `Heartbeat` or `Telemetry` is missing for N seconds, the bridge marks that drone `Stale` in `FleetState`.

`CommandAuthority` is a small unforgeable handle (a private type with a `pub(crate)` constructor) that exists only in two places: an Apply Supervisor that owns an `Approved` plan, and the safety-override path. The compiler enforces the choke point — there is no way to call `Link::upload_sortie` from a random module without having one.

## Persistence

SQLite via `sqlx`, file lives at `$STATE_DIR/oracle.db`. The schema is **fully relational** for everything that gets queried by field — status, foreign keys, summary metrics, step progress, fleet roster, audit. Geometry (mesh faces, waypoint paths) and frozen artefacts (the slicer's full `HivemindPlan` output, fleet snapshots, varying-shape amendment payloads) stay as JSON columns because they are never queried structurally and only ever read whole.

The split is deliberate: **the wire types are not the storage types**. `hivemind-protocol::Sortie` is a Rust struct that lives on the radio and in memory; the SQLite `sorties` table is a normalised projection of it tuned for the queries oracle and pantheon actually run.

```sql
-- ============================================================
-- migrations/0001_init.sql
-- ============================================================

PRAGMA foreign_keys = ON;
PRAGMA journal_mode = WAL;

-- ─── Intents ─────────────────────────────────────────────────
-- The CAD output from pantheon. One row per intent.json received.

CREATE TABLE intents (
    id              TEXT PRIMARY KEY,                    -- the scan_id from intent.json
    received_at     TEXT NOT NULL,                       -- RFC 3339
    source_file     TEXT,                                -- original mesh path, traceability
    georeferenced   INTEGER NOT NULL CHECK (georeferenced IN (0, 1)),
    constraints     TEXT NOT NULL DEFAULT '{}'           -- operator constraints JSON (varies, opaque)
);

-- One row per region marked in the intent. The triangle geometry stays opaque
-- because we never query it structurally — it only matters to the slicer.
CREATE TABLE intent_regions (
    intent_id       TEXT NOT NULL REFERENCES intents(id) ON DELETE CASCADE,
    id              TEXT NOT NULL,                       -- region id from the intent
    name            TEXT NOT NULL,
    area_m2         REAL NOT NULL,
    face_count      INTEGER NOT NULL,
    paint_spec      TEXT,                                -- JSON, optional per-region override
    faces           TEXT NOT NULL,                       -- triangle array as JSON (opaque)
    PRIMARY KEY (intent_id, id)
);

-- ─── Plans ───────────────────────────────────────────────────
-- One row per plan. The slicer's frozen output (HivemindPlan) lives in `body`
-- as JSON because it's never queried by field — but everything queryable is
-- modelled relationally below (sorties, steps, warnings, errors).

CREATE TABLE plans (
    id                              TEXT PRIMARY KEY,    -- uuid v7, sortable
    intent_id                       TEXT NOT NULL REFERENCES intents(id),
    status                          TEXT NOT NULL CHECK (status IN (
                                        'Draft', 'Proposed', 'Approved',
                                        'Executing', 'Paused',
                                        'Aborted', 'Complete', 'Failed'
                                    )),
    created_at                      TEXT NOT NULL,
    proposed_at                     TEXT,
    approved_at                     TEXT,
    approved_by                     TEXT,                -- "operator:alice" | "operator:local"
    started_at                      TEXT,
    completed_at                    TEXT,

    body_hash                       TEXT NOT NULL,       -- sha256(body), for If-Match approval
    body                            TEXT NOT NULL,       -- full HivemindPlan JSON (frozen slicer output)
    fleet_snapshot                  TEXT NOT NULL,       -- input snapshot the slicer ran against, JSON

    -- Indexed summary fields the API queries directly without parsing body:
    coverage_total_area_m2          REAL NOT NULL,
    coverage_overlap_pct            REAL NOT NULL,
    schedule_total_duration_s       INTEGER NOT NULL,
    schedule_peak_concurrent_drones INTEGER NOT NULL,
    resources_paint_ml              REAL NOT NULL,
    resources_battery_cycles        INTEGER NOT NULL,
    resources_total_flight_time_s   INTEGER NOT NULL
);
CREATE INDEX plans_status_idx     ON plans(status);
CREATE INDEX plans_created_at_idx ON plans(created_at DESC);
CREATE INDEX plans_intent_idx     ON plans(intent_id);

-- Warnings produced by the slicer for this plan. Queryable so the operator UI
-- can filter "show me only plans with critical warnings."
CREATE TABLE plan_warnings (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    plan_id         TEXT NOT NULL REFERENCES plans(id) ON DELETE CASCADE,
    severity        TEXT NOT NULL CHECK (severity IN ('info', 'warn', 'critical')),
    code            TEXT NOT NULL,                       -- e.g. 'wind_forecast_high'
    message         TEXT NOT NULL,
    context         TEXT                                 -- JSON, varies by code
);
CREATE INDEX plan_warnings_plan_idx ON plan_warnings(plan_id);

-- Errors that prevent the plan from being approvable. Same shape as warnings
-- but a different table because the semantics differ — errors are blockers.
CREATE TABLE plan_errors (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    plan_id         TEXT NOT NULL REFERENCES plans(id) ON DELETE CASCADE,
    code            TEXT NOT NULL,                       -- e.g. 'non_planar_region'
    message         TEXT NOT NULL,
    context         TEXT
);
CREATE INDEX plan_errors_plan_idx ON plan_errors(plan_id);

-- ─── Sorties + Steps ─────────────────────────────────────────
-- One row per per-drone sortie within a plan. Queryable by drone, status, plan.

CREATE TABLE sorties (
    id                      TEXT PRIMARY KEY,
    plan_id                 TEXT NOT NULL REFERENCES plans(id),
    drone_id                TEXT NOT NULL REFERENCES drones(id),
    sortie_index            INTEGER NOT NULL,            -- order within the plan
    status                  TEXT NOT NULL CHECK (status IN (
                                'Pending', 'Uploaded', 'Executing',
                                'Complete', 'Failed', 'Aborted'
                            )),
    paint_volume_ml         REAL NOT NULL,
    expected_duration_s     INTEGER NOT NULL,
    uploaded_at             TEXT,
    started_at              TEXT,
    ended_at                TEXT,
    failure_reason          TEXT,
    UNIQUE (plan_id, sortie_index)
);
CREATE INDEX sorties_plan_idx     ON sorties(plan_id);
CREATE INDEX sorties_drone_idx    ON sorties(drone_id);
CREATE INDEX sorties_status_idx   ON sorties(status);

-- One row per step within a sortie. The waypoint path stays opaque (geometry,
-- never queried structurally), everything else is properly typed.
CREATE TABLE sortie_steps (
    sortie_id                       TEXT NOT NULL REFERENCES sorties(id) ON DELETE CASCADE,
    step_index                      INTEGER NOT NULL,
    step_type                       TEXT NOT NULL CHECK (step_type IN (
                                        'Takeoff', 'Transit', 'SprayPass',
                                        'RefillApproach', 'RefillWait',
                                        'ReturnToBase', 'Land'
                                    )),
    waypoint_lat                    REAL NOT NULL,
    waypoint_lon                    REAL NOT NULL,
    waypoint_alt_m                  REAL NOT NULL,
    waypoint_yaw_deg                REAL,
    speed_m_s                       REAL NOT NULL,
    spray                           INTEGER NOT NULL CHECK (spray IN (0, 1)),
    radio_loss_behaviour            TEXT NOT NULL CHECK (radio_loss_behaviour IN (
                                        'Continue', 'HoldThenRtl', 'RtlImmediately'
                                    )),
    radio_loss_silent_timeout_s     REAL NOT NULL,
    radio_loss_hold_then_rtl_after_s REAL,
    expected_duration_s             INTEGER NOT NULL,
    path                            TEXT,                -- waypoint array as JSON, opaque
    PRIMARY KEY (sortie_id, step_index)
);

-- Step progress is the resume-after-crash data path. The Apply Supervisor
-- writes one row per (sortie, step) and updates it through the lifecycle.
-- Restart-resume reads the latest row per (sortie, step) for any plan in
-- 'Executing' status and picks up from there.
CREATE TABLE step_progress (
    sortie_id               TEXT NOT NULL,
    step_index              INTEGER NOT NULL,
    state                   TEXT NOT NULL CHECK (state IN (
                                'Gating', 'Running', 'Complete',
                                'Failed', 'Held', 'Aborted'
                            )),
    gate_decision           TEXT CHECK (gate_decision IN (
                                'AutoProceed', 'OperatorRequired',
                                'FleetConflict', 'AbortSortie'
                            )),
    gate_reason             TEXT,
    gated_at                TEXT,
    started_at              TEXT,
    completed_at            TEXT,

    -- Telemetry snapshot at the moment of completion (or failure)
    position_lat            REAL,
    position_lon            REAL,
    position_alt_m          REAL,
    battery_pct             REAL,
    paint_remaining_ml      REAL,
    duration_s              REAL,
    failure_reason          TEXT,

    PRIMARY KEY (sortie_id, step_index),
    FOREIGN KEY (sortie_id, step_index) REFERENCES sortie_steps(sortie_id, step_index)
);
CREATE INDEX step_progress_state_idx ON step_progress(state);

-- ─── Fleet roster ─────────────────────────────────────────────
-- Long-lived per-drone state. Updated whenever telemetry arrives.

CREATE TABLE drones (
    id                          TEXT PRIMARY KEY,        -- the drone_id from legion's Hello
    first_seen_at               TEXT NOT NULL,
    last_seen_at                TEXT NOT NULL,
    legion_version              TEXT,
    capabilities                TEXT,                    -- JSON array
    last_known_battery_pct      REAL,
    last_known_paint_ml         REAL,
    last_known_position_lat     REAL,
    last_known_position_lon     REAL,
    last_known_position_alt_m   REAL,
    last_known_drone_phase      TEXT CHECK (last_known_drone_phase IN (
                                    'Idle', 'Armed', 'InAir',
                                    'ExecutingStep', 'Holding', 'Landing'
                                )),
    is_stale                    INTEGER NOT NULL DEFAULT 0 CHECK (is_stale IN (0, 1))
);
CREATE INDEX drones_last_seen_idx ON drones(last_seen_at DESC);

-- ─── Amendments ──────────────────────────────────────────────
-- Plan modifications applied during execution. Stored relationally for
-- "show me everything that happened to plan X" queries.

CREATE TABLE amendments (
    id                  TEXT PRIMARY KEY,
    plan_id             TEXT NOT NULL REFERENCES plans(id),
    kind                TEXT NOT NULL,                   -- e.g. 'DroneDown', 'SkipRegion'
    requires_approval   INTEGER NOT NULL CHECK (requires_approval IN (0, 1)),
    applied_at          TEXT NOT NULL,
    operator            TEXT,
    body                TEXT NOT NULL                    -- JSON, varies by kind
);
CREATE INDEX amendments_plan_idx ON amendments(plan_id);

-- ─── Audit log ───────────────────────────────────────────────
-- Append-only. Every command sent, every approval, every safety hold,
-- every step gate decision, every amendment. The artefact a SORA-trained
-- accident investigator reads after an incident.

CREATE TABLE audit_log (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    ts              TEXT NOT NULL,
    actor           TEXT NOT NULL,                       -- "operator:alice" | "system:fleet_monitor" | "system:legion_link"
    event           TEXT NOT NULL,                       -- typed event name (e.g. 'plan_approved', 'step_proceed', 'conflict_hold', 'sortie_uploaded')
    plan_id         TEXT REFERENCES plans(id),
    sortie_id       TEXT REFERENCES sorties(id),
    drone_id        TEXT,
    payload         TEXT NOT NULL DEFAULT '{}'           -- structured event payload as JSON
);
CREATE INDEX audit_log_ts_idx     ON audit_log(ts);
CREATE INDEX audit_log_plan_idx   ON audit_log(plan_id);
CREATE INDEX audit_log_sortie_idx ON audit_log(sortie_id);
CREATE INDEX audit_log_actor_idx  ON audit_log(actor);
CREATE INDEX audit_log_event_idx  ON audit_log(event);
```

### What stays JSON, and why

| Column | Why JSON |
|---|---|
| `intents.constraints` | Operator constraints — varies, oracle passes them to the slicer untouched, never queried by field |
| `intent_regions.faces` | Triangle geometry — only the slicer reads it, and only as a whole |
| `plans.body` | The full `HivemindPlan` — frozen slicer output, always read whole, never queried by field. Indexed summary fields above sit alongside it for queries. |
| `plans.fleet_snapshot` | Point-in-time snapshot the slicer ran against. Frozen, opaque, never queried structurally. Stored to make plan reproduction possible. |
| `sortie_steps.path` | Waypoint array for `Transit` / `SprayPass` segments. Geometry, never queried structurally. |
| `amendments.body` | Varies by `kind`. `DroneDown` has different fields from `SkipRegion`. JSON keeps the schema sane. |
| `audit_log.payload` | Varies by `event`. JSON keeps the audit log universal. |

### What this gives us

- **Restart-resume** works because `step_progress` is the canonical state, written on every transition. On boot, `SELECT * FROM step_progress WHERE sortie_id IN (SELECT id FROM sorties WHERE status = 'Executing')` gives the supervisor everything it needs.
- **The audit log is queryable.** "Show me every command sent to drone-01 between 14:00 and 15:00" is one indexed query, not a JSON-blob scan.
- **Plans can be filtered cheaply.** Status, intent, creation time, peak concurrent drones, total paint — all indexed columns. "Show me approved plans for this intent" is a single index lookup.
- **Foreign keys catch dangling state.** Deleting an intent cascades to its regions, plans, sorties, steps, and progress. (The audit log keeps the dangling references for forensics — `audit_log.plan_id` is a soft FK via `REFERENCES plans(id)` but with no `ON DELETE` clause, so the audit row stays even if the plan is deleted.)
- **`sqlx` macro queries** type-check at compile time against this schema. Schema drift is a build error, not a runtime mystery.

## HTTP / WebSocket API

```
POST   /v1/intents                                      ← upload intent.json from pantheon
GET    /v1/intents/:id

POST   /v1/plans                                        ← body: { intent_id, planner_options } → runs plan(), returns HivemindPlan
GET    /v1/plans/:id
GET    /v1/plans?status=Proposed

POST   /v1/plans/:id/approve                            ← Proposed → Approved, spawns Apply Supervisor
POST   /v1/plans/:id/abort                              ← any → Aborted, RTLs all drones in flight for the plan
POST   /v1/plans/:id/amendments                         ← body: PlanAmendment
POST   /v1/plans/:id/replan                             ← body: { planner_options } → produces a new Plan with diff vs. this one

POST   /v1/plans/:id/sorties/:sortie/steps/:idx/proceed ← operator approval for a gated step
POST   /v1/plans/:id/sorties/:sortie/steps/:idx/hold    ← operator-driven step hold
POST   /v1/plans/:id/sorties/:sortie/abort              ← abort just this sortie (legion RTLs that drone)

GET    /v1/fleet                                        ← snapshot of every drone known to the legion link
GET    /v1/fleet/:drone_id

GET    /v1/audit?since=…                                ← audit-log slice for replay/debug

GET    /ws/telemetry                                    ← WebSocket: pantheon subscribes to TelemetryEvent broadcast

(The legion link is *not* an HTTP endpoint. It's a separate Transport opened
 directly on the truck's serial radio or a TCP listener — see the Legion Link
 section above.)
```

Conventions:

- All bodies JSON, all timestamps RFC 3339, all enums in PascalCase to match the Rust types.
- Errors are `application/problem+json` with stable `type` URIs so pantheon can switch on them.
- The plan-approval endpoint accepts an `If-Match` header carrying the plan's `body_hash`. If the operator's pantheon is showing a stale version of the plan, the approval fails — they cannot approve something they aren't actually looking at.
- The pantheon-facing `/ws/telemetry` sends `TelemetryEvent` JSON frames at whatever rate the legion link produces them, with an internal floor at ~5 Hz per drone for position updates so a slow client doesn't miss everything.

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

## CLI

Oracle ships a `hivemind` CLI that mirrors the same plan/apply flow. This lets oracle be developed and tested *before* pantheon's UI is ready, and gives field engineers a no-frills fallback:

```bash
$ hivemind serve                                # runs the daemon
$ hivemind plan --intent intent.json --drones 6 # upload intent + create plan; pretty-prints the proposal
$ hivemind apply <plan-id>                      # POST .../approve, streams progress
$ hivemind status                               # GET /v1/fleet + active plan summary
$ hivemind proceed <plan-id> <sortie> <step>    # operator proceed for a gated step (debug aid)
$ hivemind abort                                # POST .../abort on the active plan
$ hivemind audit --since 1h                     # tail the audit log
```

The CLI subcommands (other than `serve`) are thin clients over the daemon's HTTP API on a Unix domain socket at `$STATE_DIR/oracle.sock`. The CLI never talks to legion directly. It's not a throwaway — it stays as a debug and recovery tool for the life of the project.

## Configuration

A single `oracle.toml`, env-overlayable:

```toml
[server]
http_addr = "127.0.0.1:7345"
unix_socket = "/var/run/oracle/oracle.sock"

[storage]
state_dir = "/var/lib/oracle"

[legion_link]
# v1 production: serial radio on the truck. Swap kind = "tcp" for SITL/dev.
kind = "serial"
serial_path = "/dev/ttyUSB0"
serial_baud = 57600
# Used only when kind = "tcp"
tcp_listen = "0.0.0.0:7346"

# Allowlist pins which drone IDs are accepted in the Hello exchange;
# everyone else is rejected and the session is dropped.
allowed_drones = ["drone-01"]
shared_token = "${ORACLE_LEGION_TOKEN}"
heartbeat_to_legion_hz = 2
legion_heartbeat_timeout_ms = 3000
# How long an operator-required gate waits before timing out the plan
operator_gate_timeout_s = 600

[rtk]
# RTCM3 source on the truck. Oracle reads from this and forwards to every connected
# legion as RtkCorrection frames. Legion injects into the Pixhawk locally via rust-mavlink.
source = "serial:/dev/ttyUSB-rtk:115200"   # or "ntrip://..." for an NTRIP caster
broadcast_hz = 1

[slicer]
spray_width_m = 0.30
overlap_pct = 0.20
min_horizontal_separation_m = 3.0
battery_safety_margin_pct = 25
paint_safety_margin_pct = 15

[safety]
fleet_monitor_hz = 5
```

Env vars use `ORACLE__` prefix with double-underscore section delimiters (figment convention).

## Error handling

- Library code returns `Result<T, SlicerError>` / `Result<T, LinkError>` etc. (`thiserror`).
- HTTP handlers map those to `application/problem+json` via a single `IntoResponse` impl on a top-level `ApiError` enum.
- The Apply Supervisor never panics; every error transitions the plan to `Failed` with a captured error chain in the audit log, then sends `AbortSortie` to every drone currently in the air for that plan (which in turn triggers legion's RTL).
- `tracing` spans wrap every plan id, sortie id, and step index so logs can be filtered post-incident.

## Testing strategy

In order of cost:

1. **Unit tests** for the slicer modules. `coverage`, `lanes`, `sortie_pack`, `steps`, `radio_loss` are pure functions over plain types — they test in milliseconds.
2. **Snapshot tests** (`insta`) over `HivemindPlan` for a small library of fixture intents (one flat region, two adjacent regions, a region with an unreachable face, an intent with conflicting constraints, an intent that should produce per-step `RtlImmediately` policies). These are the regression net.
3. **Apply-handshake tests** that drive the Apply Supervisor against a Rust mock legion in `tests/`. The mock links the same `hivemind-protocol` crate, opens a `TcpTransport` (or a `socat` pseudo-tty pair for the serial path), and scripts canned executions: "ack the upload, complete step 0 in 200 ms, complete step 1 in 5 s, fail step 2 with reason X." The supervisor's gate decisions and persisted state are asserted at every step. With both sides in Rust, wire-format drift is a `cargo check` error — no cross-language protocol tests needed.
4. **API smoke tests** that spin up the axum app in a `tokio::test`, POST a fixture intent, walk plan → approve → handshake → abort, and assert the audit-log shape. These run on every CI run.
5. **Schema migration tests.** Each `sqlx` migration runs forward and is queried via the same compile-time-checked macros the binary uses. Adding a column means re-running the macro check.
6. **(Optional CI gate) full pair test.** Real legion + real oracle + PX4 SITL. Slow, nightly, but the only test that exercises the entire stack at once.

## What v1 ships, what it doesn't

**v1 ships:**

- The single binary, the HTTP+WS API to pantheon, the CLI, SQLite persistence with the relational schema above, the audit log.
- The slicer, end-to-end, for flat regions on a single drone — including step assembly and per-step radio-loss policy assignment.
- The Apply Supervisor with the full step-confirmation handshake and gate decision tree.
- Restart-resume from `step_progress`.
- The Legion Link with `SerialTransport` over a SiK-class radio (v1 production) and `TcpTransport` for SITL/dev.
- The `hivemind-protocol` shared crate, versioned at v1.0, consumed by both oracle and legion.
- RTCM3 forwarding from a serial/NTRIP RTK source to every connected legion as `RtkCorrection` frames.
- Operator step-gate endpoints (`/proceed`, `/hold`, `/abort` per step) and the corresponding pantheon events.
- The Apply Supervisor + Fleet Monitor as architecturally complete tasks even though Layer 2 is a no-op for one drone.

**v1 explicitly does not:**

- Run on more than one drone. The architecture supports multi-drone via per-drone `Session` actors from day one; v1 ships with a single-drone allowlist and no operator workflows for managing N drones.
- Generate paths on curved surfaces. `coverage::generate_passes` returns `PlanError::NonPlanarRegion`.
- Replan from a partial-completion state. `POST /plans/:id/replan` exists but for v1 only accepts a fully un-applied plan as the predecessor; mid-execution replan comes in v2.
- Speak Skybrush Server's FlockWave protocol. v1 rolls its own protocol in `hivemind-protocol` because the surface area we need is small, the typing benefits of owning the schema (one Rust crate, both binaries link it) are large, and FlockWave's ground-station orientation isn't a fit for our step-confirmation handshake. We re-evaluate when v2 wants Skybrush Live as a fallback ground station.
- Speak MAVLink at all. That's legion's job. If something in oracle ever needs a MAVLink message, the answer is to add a typed frame to `hivemind-protocol` and implement it on the legion side.
- Provide the QGroundControl-over-SiK backup channel. The hardware supports it; plumbing it through is a hardware decision tracked in [hw/](../hw/README.md), independent of the backend.

## Why this matters for the economics

The top-level [README's Economics section](../README.md#economics) makes the case that Hivemind's value comes from **deleting scaffolding, not painting faster**. Oracle is what makes that claim defensible:

- Scaffolding-free industrial work is only acceptable to clients and regulators if every drone movement is **previewable, approvable, and auditable**. The plan/apply lifecycle plus the per-step handshake plus the relational audit log is how oracle delivers that.
- Centimetre-accurate work needs the slicer to consume *current* fleet state (battery, paint, wind, RTK status). Pre-baked plans authored in pantheon would not have this. Oracle does.
- SORA approval for BVLOS swarm flight — the binding constraint on the timeline — is much easier to argue for a system where a human explicitly approves a complete, validated plan before any drone moves and gates every step transition than for a system where the operator points at a region and drones figure it out.

Oracle is not glue. It is the part of Hivemind that turns "we have drones and a 3D scan" into something a regulator and a customer will sign off on.

## Open questions

These are the things that need an answer before code lands, not after:

1. **Spray-width / overlap tuning** — these are slicer config today, but they belong in the paint spec on the intent file, per region. Worth deciding whether to extend the v1.0 intent format now or defer to v1.1.
2. **How many gates need operator approval by default?** Two extremes: every step requires `proceed` (safest, slowest, fine for v1), or only "first spray pass of the day + first approach of any new region" (faster, more autonomous, needs a clearer policy). v1 ships with a slicer config flag and starts on the safe end.
3. **`expected_step_index` mismatch handling.** If oracle sends `Proceed { expected: 4 }` while legion is on step 2 (because of a duplicate or out-of-order frame), legion rejects with an `Error`. Oracle's Apply Supervisor should treat this as a protocol bug and abort the sortie, not retry. Confirm before code lands.
4. **Operator-gate timeout policy.** What does the supervisor do if a step is in `OperatorRequired` state and the operator doesn't click for 10 minutes? v1 default: fail the sortie and RTL the drone. Configurable via `[legion_link] operator_gate_timeout_s`.
5. **Operator identity.** v1 has one operator and no auth. The audit log records `operator:local`. v2 needs at least bearer-token auth on the HTTP API; the column already exists.
6. **Intent file size limit.** Bridges with thousands of faces will produce multi-MB intents. v1 caps at 16 MB and defers the OBJ-per-region split the pantheon README mentions.
7. **Radio bandwidth budget.** A typical sortie upload is ~2–5 kB of postcard, plus per-step `Telemetry` at ~80 bytes × 2 Hz × N drones. At 57600 bps a sortie upload takes well under a second, and the steady-state telemetry budget for 10 drones is ~1.6 kB/s. Comfortable on SiK. Pushing large payloads (post-job photos, video) is what doesn't fit; for that v2 either adds an IP radio as a secondary or accepts a "wait until truck range" workflow.
8. **Workspace lints + MSRV.** New workspace, new chance to set lints (`clippy::pedantic`?), MSRV (1.75? 1.80?), and `[workspace.dependencies]` for the crates oracle and legion both want (`tokio`, `serde`, `tracing`, `thiserror`, `figment`). Decision deferred to the Cargo.toml-creation pass, but worth flagging now so we don't drift.
9. **`SerialTransport` multi-drone routing on a shared SiK channel.** SiK radios are broadcast — every air-side radio hears every ground-side frame. v1 with one drone is fine; v2 with multiple drones needs every frame on the wire to carry the destination `drone_id` so legions can ignore frames not addressed to them, and oracle has to decode every received frame to figure out which drone it came from. The protocol's `Envelope.drone_id` already exists for this. Worth re-confirming the assumption when we field the second drone.
