# legion

On-drone agent. One Rust binary per drone, running on the companion computer (a Raspberry Pi 5 alongside the Pixhawk). Legion is the boundary between the swarm orchestrator (oracle, on the truck) and the autopilot (PX4, on the drone).

> See the top-level [README](../README.md) for project context, [oracle/README.md](../oracle/README.md) for the other end of the wire, and [protocol/README.md](../protocol/README.md) for the canonical wire-format reference.

## Role

Legion is **not** a swarm comms layer. It does not talk to other legions, it does not route packets, it does not participate in distributed planning. Each legion knows about exactly one drone — its own — and exactly one peer — oracle. The parent README's star topology is enforced here, on the drone side.

Concretely, legion:

- Receives full *sorties* from oracle in one upload over the radio link.
- Stores them locally so a radio drop doesn't lose state.
- Executes them step by step, requesting `Proceed` from oracle between every step.
- Falls back to a per-step *radio loss policy* if oracle stops answering mid-step.
- Drives PX4 via `rust-mavlink` over the Pixhawk's TELEM2 UART.
- **Commands the nozzle servo via MAVLink to Pixhawk AUX5** (not a Pi-side actuator — see [hw/nozzle](../../hw/nozzle/README.md) and the [Nozzle control section](#nozzle-control--pixhawk-aux5-via-mavlink) below). The "spray on/off" operation is one `MavlinkBackend::set_nozzle` call.
- Owns the forward ToF sensor (wired to the Pi's I²C — hardware the Pixhawk doesn't see).
- Runs a 10 Hz local safety loop that preempts the executor if anything goes wrong.
- Streams telemetry back to oracle at 2 Hz.

The clean responsibility split:

| Concern | Owner |
|---|---|
| Plan / schedule / deconflict the swarm | **oracle** |
| Approve plans, gate step transitions | **oracle** + operator (via pantheon) |
| Execute one drone's sortie step by step | **legion** |
| Forward ToF sensor (Pi I²C) | **legion** |
| Nozzle servo (Pixhawk AUX5) | **PX4**, commanded by legion via MAVLink |
| Local safety loop (ToF, battery, paint, oracle watchdog) | **legion** |
| Stabilisation, motor mixing, waypoint following | **PX4** |
| MAVLink, the wire | **legion ↔ PX4 only** |

The oracle ↔ legion link is **not** MAVLink — see ["Why not MAVLink for this link"](#why-not-mavlink-for-the-oracle--legion-link) below.

## Topology

```
   ┌── Truck NUC (Rust binary) ──────────────┐
   │                                         │
   │  ┌──────────────┐                       │
   │  │   oracle     │                       │
   │  └──────┬───────┘                       │
   │         │ hivemind-protocol             │
   │         │ (postcard frames over COBS)   │
   │         ▼                               │
   │  ┌──────────────┐    UART/USB           │
   │  │ SiK base     │◀─────────             │
   │  │ (HolyBro 433 │                       │
   │  │  v1 / RFD868x│                       │
   │  │  v2)         │                       │
   │  └──────────────┘                       │
   └─────────┬───────────────────────────────┘
             │
             │ EU SRD radio link
             │ v1: 433 MHz ISM, 100 mW, ~5 km LOS, ~57600 bps effective
             │ v2: 869.4–869.65 MHz SRD-d, 500 mW EIRP, ~40 km, same UART
             │
   ┌─────────┼─────────────────────────────────────────┐
   │         ▼                                         │
   │  ┌──────────────┐    UART                         │
   │  │ SiK air      ├───────────┐                    │
   │  │ (paired)     │           │                    │
   │  └──────────────┘           │                    │
   │                             │                    │
   │  ┌──────────────────────────┴───────────┐        │
   │  │            Pi 5  (legion)            │        │
   │  │                                      │        │
   │  │  ┌─────────┐    I²C    ┌──────────┐  │        │
   │  │  │ ToF     ├───────────│ VL53L1X  │  │        │
   │  │  │ driver  │           │ (fwd ToF)│  │        │
   │  │  │ (rppal) │           └──────────┘  │        │
   │  │  └─────────┘                         │        │
   │  │                                      │        │
   │  │  ┌─────────┐   UART    ┌──────────┐  │        │
   │  │  │ rust-   ├───────────│  Pixhawk │  │        │
   │  │  │ mavlink │  TELEM2   │   (PX4)  │  │        │
   │  │  └─────────┘           │          │  │        │
   │  └────────────────────────┤          │  │        │
   │                           │  AUX5 ───┼──┼──┐     │
   │                           └────┬─────┘  │  │     │
   │                                │        │  ▼     │
   │                                │ ESC    │ ┌────┐ │
   │                                │ bus    │ │SG90│ │
   │                                ▼        │ │servo│ │
   │                          ┌──────────┐   │ └─┬──┘ │
   │                          │  motors  │   │   ▼    │
   │                          └──────────┘   │ ┌────┐ │
   │                                         │ │spray│ │
   │                                         │ │ can │ │
   │                                         │ └────┘ │
   │  Drone N                                │        │
   └─────────────────────────────────────────┴────────┘

   Optional independent backup channel (parallel, not part of legion or oracle):

   ┌── Truck NUC ──┐                    ┌── Drone N ──┐
   │ QGroundControl│◀── 2nd SiK link ───│  Pixhawk    │
   └───────────────┘    MAVLink         │  TELEM1     │
                                        └─────────────┘
```

Things to notice:

1. **The radio is a serial UART, not a network.** v1 ships with EU-legal SiK-class telemetry radios — the **HolyBro SiK Telemetry Radio V3 (433 MHz EU variant)** for the cheap v1 budget, with **RFDesign RFD868x** as the v2 production target running in the 869.4–869.65 MHz SRD-d sub-band (the EU slot designed for high-power short-range telemetry, 500 mW EIRP, 10% duty cycle). Both run the SiK firmware family and look like `/dev/ttyUSB0` on the truck and a UART on the Pi. There is no IP layer, no TCP, no WebSocket. Our wire protocol runs as binary frames directly on top of the radio's serial byte stream. **No 915 MHz hardware** — that's US/Canada-only and illegal to operate in the EU.
2. **Two MAVLink consumers of the same Pixhawk are possible.** v1 uses TELEM2 for legion (UART, low latency, no contention). An optional second SiK on TELEM1 can connect to QGroundControl on the truck for an *independent* monitoring/manual-override channel — plumbed at the hardware layer, not in legion or oracle.
3. **Legion never talks MAVLink to anything other than its own Pixhawk.** No mavlink-router, no fan-out, no shared MAVLink session with oracle. The radio carries our protocol, not MAVLink.
4. **Hardware that the Pixhawk doesn't see lives on legion.** For v1 that's the forward ToF sensor (VL53L1X, wired to the Pi's I²C and driven by legion via `rppal`). The **nozzle servo is a Pixhawk actuator** on AUX5 — legion commands it by sending MAVLink `MAV_CMD_DO_SET_SERVO` over TELEM2, not by toggling a Pi GPIO. See [hw/nozzle](../../hw/nozzle/README.md) and the [Nozzle control section](#nozzle-control--pixhawk-aux5-via-mavlink) below for why.

## Why not MAVLink for the oracle ↔ legion link

MAVLink is built for **flight controller ↔ ground station** communication. The oracle ↔ legion link is doing something different: high-level swarm coordination, sortie uploads, step gating, payload state, plan/apply auditing. Those are application-level concerns that MAVLink wasn't designed for.

| | MAVLink | Custom protocol |
|---|---|---|
| Message types | ~300 predefined drone messages | Whatever we need |
| Custom data (sortie, paint, ToF, step gating) | Tunnelled through `MAV_CMD_USER_*` and a custom dialect — works but ugly | First-class fields in our own types |
| Step-confirmation handshake | No native concept | Native to the protocol |
| Acknowledgment richness | Basic ACK/NACK | "Step 3 complete, paint at 320 ml, battery 64%, took 47 s" |
| Sortie upload | Item-by-item `MISSION_REQUEST` handshake — bandwidth-hostile on a 57600 baud radio | One frame, validated, persisted on receipt |
| Operator audit trail | Hard | Trivial — every frame logged with structured fields |
| Inspecting on the wire | binary, needs decoders | binary, but `--protocol-debug` mode can pretty-print at either end |

The cost of using MAVLink for this is the wrong abstraction at every step; the cost of our own protocol used to be "two repos that have to mirror types," but with both ends in Rust and a shared `hivemind-protocol` crate, that cost is gone.

**MAVLink stays where it belongs:** between legion and the Pixhawk (UART, on the same drone, never on the radio).

## Nozzle control — Pixhawk AUX5 via MAVLink

The v1 spray mechanism is an SG90 servo pressing a standard aerosol can's nozzle button — full build doc in [hw/nozzle](../../hw/nozzle/README.md). The servo is wired to **Pixhawk AUX5**, not to a Pi GPIO. legion commands it via MAVLink (`MAV_CMD_DO_SET_SERVO` on servo index 5, PWM 2000/1000), the same way it commands any other Pixhawk actuator.

Why AUX5 and not Pi GPIO:

- **Single control path.** Every actuator on the drone (motors, ESCs, the nozzle servo, any future gimbal) lives on the flight controller. Adding a second control path (Pi PWM for the nozzle, Pixhawk PWM for everything else) would mean legion has two kinds of drivers and two kinds of failure modes for one drone.
- **Deterministic timing.** PX4's PWM output is generated on the flight controller's hardware timers. Pi-side software PWM through rppal is kernel-scheduled and jitters under load.
- **Reboot safety.** A Pi reboot mid-sortie must not change the nozzle position. Pixhawk-driven outputs stay latched at their last commanded PWM.
- **Failsafe coupling.** PX4's existing failsafe logic (battery, GPS loss, RC loss) can already force AUX5 to a safe position. Pi GPIO wouldn't inherit that.

So legion's `MavlinkBackend` trait has a single `set_nozzle(open: bool)` method, and that's the *only* way spray is commanded anywhere in the codebase — the executor calls it at spray-step boundaries, the safety loop calls it on any trip, and the radio-loss policy calls it before unwinding. There is no Pi-side `Pump` or `Nozzle` trait.

```rust
// Every spray call in legion-core funnels through this one method.
mavlink.set_nozzle(true).await?;   // AUX5 → PWM 2000 → servo pressed → spray on
mavlink.set_nozzle(false).await?;  // AUX5 → PWM 1000 → servo released → spray off
```

PX4 parameters (set once via QGroundControl):

```
AUX5 function = "Servo"
PWM_AUX_MIN5  = 1000
PWM_AUX_MAX5  = 2000
```

`hw/nozzle/README.md` is the canonical reference for the wiring (3 wires: GND / 5 V / signal), the mounting, the testing procedure, and the PX4 params. legion's `MavlinkBackend` impl is the only code that touches the actuator command.

## Stack at a glance

| Concern | Choice | Why |
|---|---|---|
| Language | **Rust 2021** | Same as oracle. Single static binary on the Pi, no Python/MAVSDK-server footprint, lifetimes catch the kind of safety/concurrency bugs that would otherwise crash a drone. The parent README's "few hundred lines" target still holds — Rust is more verbose than Python but the core executor is small. |
| Async runtime | **tokio (multi-thread)** | Same as oracle. Required for the transport, the MAVLink driver, and the parallel safety/executor/comms tasks. |
| Autopilot driver | **`mavlink` crate (rust-mavlink)** directly | Pure Rust, supports MAVLink 1 & 2, code-generated typed message structs from `common.xml`. Connects to the Pixhawk via `tokio-serial` on TELEM2. We avoid the Rust `mavsdk` crate (which talks to a separate C++ `mavsdk-server` over gRPC) because it would drag the gRPC stack and a C++ binary onto the Pi for marginal benefit at v1 scale. |
| Drone link to oracle | **`hivemind-protocol`** crate (shared workspace member) | Defines all message types, the `Transport` trait, and the postcard+COBS framing. Legion implements the client side; oracle implements the server side. Same bytes on the wire. |
| Wire format | **postcard** (binary, serde-driven) + **COBS** framing | Postcard is the de-facto compact binary format for embedded Rust. COBS gives self-synchronising frame boundaries on a serial byte stream — a single byte loss only loses one frame, not the whole stream. Both transports (serial + TCP) use the same bytes. |
| Radio transport | **`tokio-serial`** for v1/v2 SiK-class radios (HolyBro SiK 433 MHz for v1, RFD868x for v2); **plain `tokio::net::TcpStream`** swappable for SITL / dev / IP-based future radios | Both back the same `Transport` trait. v1 + v2 production hardware is serial; the TCP impl exists for tests, SITL, and the eventual IP-radio option. **EU-legal frequencies only** — no 915 MHz parts. |
| Pi peripherals (I²C for the ToF sensor) | **`rppal`** | Pure-Rust Pi peripheral library. v1 only uses it for the forward VL53L1X over I²C; the nozzle servo lives on Pixhawk AUX5, not on Pi GPIO, so there is no Pi-side PWM in the flight path. Async-friendly when blocking calls are wrapped in `spawn_blocking`. |
| Persistence | **`serde_json` + atomic file writes** in `/var/lib/legion/` | One file per sortie, atomic via `tempfile + std::fs::rename`. SQLite would be overkill — we have at most a handful of sortie files at a time. |
| Logging | **tracing + tracing-journald** | Same as oracle. Structured spans, journald output, post-job export. |
| Process supervisor | **systemd unit** | Legion runs as a system service on the Pi (`legion.service`). Auto-restart on crash, journald logs shipped off the drone post-job. |
| Errors | **`thiserror`** in modules, **`anyhow`** at the binary edge | Same split as oracle. |
| Time / IDs | **`time`** + **`uuid` v7** | Sortable IDs, no `chrono` cve churn. Same as oracle. |
| Config | **`figment`** (TOML + env overlays) | Same as oracle. |
| Tests | **`cargo test`**, **`tokio::test`** for async cases, mock `Payload` + `MavlinkBackend` impls (the latter records `set_nozzle` calls for spray assertions), **PX4 SITL** as a nightly gate | Pure-Rust test stack. Hardware mocks are trait impls; the executor unit-tests against fakes in milliseconds. |

## Workspace structure

With both oracle and legion in Rust — and the eventual MCU swap in mind (see below) — the natural shape is a Cargo workspace at the top of the repo with **four** Rust members:

```
hivemind/
├── Cargo.toml                   ← workspace root: lists members, shared deps, lints
├── README.md                    ← project overview (existing)
│
├── protocol/                    ← shared crate: hivemind-protocol  (no_std + alloc)
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs               ← re-exports
│       ├── messages.rs          ← OracleToLegion, LegionToOracle enums
│       ├── sortie.rs            ← Sortie, SortieStep, RadioLossPolicy, StepType
│       ├── telemetry.rs         ← Telemetry, StepComplete, SortieEvent, SafetyEvent
│       ├── transport.rs         ← Transport trait + COBS-postcard codec
│       ├── tcp.rs               ← TcpTransport impl (feature = "tcp", std-only)
│       └── serial.rs            ← SerialTransport impl (feature = "serial", std-only)
│
├── legion-core/                 ← portable executor + safety + hardware traits  (no_std + alloc)
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── executor/            ← state machine, step handlers, generic over traits
│       ├── safety/              ← single-tick safety check, generic over traits
│       ├── radio_loss.rs        ← per-step policy enforcement
│       ├── traits/              ← Payload, MavlinkBackend, SortieStore, Clock — *definitions only*
│       └── error.rs
│
├── oracle/                      ← truck-side runtime  (std + tokio + axum + sqlx)
│   ├── README.md
│   ├── Cargo.toml               ← workspace member; depends on hivemind-protocol
│   └── src/
│
├── legion/                      ← Pi-side runtime  (std + tokio + rppal + rust-mavlink)
│   ├── README.md                ← this file
│   ├── Cargo.toml               ← workspace member; depends on legion-core + hivemind-protocol
│   ├── systemd/legion.service
│   └── src/
│
├── (future) legion-mcu/         ← MCU-side runtime  (no_std + embassy + embedded-hal)
│   └── ...
│
├── pantheon/                    ← Blender add-on, not Rust, not in the workspace
└── ...
```

The shared crates (`protocol`, `legion-core`) are the one place wire types and execution logic live. The Pi-side `legion` crate provides concrete hardware impls; a future `legion-mcu` crate provides MCU-side concrete impls. Both consume `legion-core` and `hivemind-protocol` unchanged.

## Designing for the MCU swap

The Pi 5 is the v1/v2 companion-computer target. Long term, we may swap to a Cortex-M-class MCU running [embassy](https://embassy.dev/) to save weight, power, and bring-up time. The architecture is structured so that swap is a *new crate*, not a rewrite of legion.

The key constraint: **`legion-core` is `#![no_std]` (with `extern crate alloc;`)**. That means inside the executor and safety loop:

- **No `std::sync` types** (`Arc`, `Mutex`, `RwLock`). State sharing happens via generics — the binary that hosts `legion-core` decides whether the concrete `LegionState` lives behind a `tokio::sync::RwLock` (Pi) or a `embassy_sync::blocking_mutex::CriticalSectionMutex` (MCU).
- **No `tokio`**. The core uses `core::future::Future`, `core::time::Duration`, and stable `async fn` in trait. Both tokio and embassy can drive these futures.
- **No file I/O**. Persistence is a `SortieStore` trait. The Pi binary supplies a file-backed impl; the MCU supplies a flash-backed impl (or a no-op `NullStore`).
- **No allocations in hot loops** that aren't obviously bounded. The safety loop reads sensors and writes commands — no allocs at all. The executor allocates a small `Vec` for the step iterator, fine.
- **`alloc::vec::Vec`, `alloc::string::String`, `alloc::boxed::Box` are OK** because realistic MCU targets (Cortex-M4F/M7 with ≥256 KB RAM) can run a small global allocator (`linked_list_allocator`, `embedded-alloc`).

What's portable, in `legion-core`:

| Module | Purpose | Generic over |
|---|---|---|
| `executor::Executor<P, M, S, T>` | Walks a Sortie step by step, runs the `Proceed` handshake, dispatches to step handlers | `Payload, MavlinkBackend, SortieStore, Transport` |
| `executor::steps` | Per-`StepType` handlers (Takeoff, Transit, SprayPass, …) | `Payload, MavlinkBackend` |
| `executor::radio_loss::apply` | Per-step policy enforcement (Continue / HoldThenRtl / RtlImmediately) | `MavlinkBackend, Transport` |
| `safety::check` | Single-iteration safety check, returns `SafetyOutcome`. The 10 Hz loop wrapper lives in the binary. | `Payload, MavlinkBackend, Clock` |
| `traits::*` | Trait definitions only — no impls | — |

What stays binary-specific, in `legion`:

| Module | Why |
|---|---|
| `main.rs`, clap CLI, figment config loading | std-only |
| `tracing` setup, journald output | std-only (MCU uses `defmt`) |
| `RppalTof` | rppal is std-only and Pi-specific; v1 uses it only for the VL53L1X over I²C |
| `RustMavlinkDriver` (impls `MavlinkBackend`) | uses `tokio-serial` and the std features of the `mavlink` crate |
| `TcpTransport` / `SerialTransport` | from `hivemind-protocol`, behind feature flags, std-only |
| `FileSortieStore` | uses `std::fs` |
| The tokio runtime + the wrapper loops that call `legion_core::safety::check` and `legion_core::executor::Executor::run` | std-only |

Sketch of the trait surface in `legion-core`:

```rust
// legion-core/src/traits/payload.rs
//
// Pi-side sensors only. The nozzle is NOT here — it's on Pixhawk
// AUX5 and lives on MavlinkBackend::set_nozzle. See the "Nozzle
// control — Pixhawk AUX5 via MAVLink" section above for rationale.

pub trait Tof: Send {
    async fn read_cm(&mut self) -> Result<f32, PayloadError>;
}

pub trait PaintLevel: Send {
    /// v1 returns `PayloadError::NotInstalled` — there's no HX711
    /// load cell on the cheap-build aerosol payload (see
    /// `hw/nozzle/README.md`). v2 swaps in a real impl.
    async fn read_ml(&mut self) -> Result<f32, PayloadError>;
}

pub trait Payload {
    type Tof: Tof;
    type PaintLevel: PaintLevel;
    fn tof(&mut self) -> &mut Self::Tof;
    fn paint_level(&mut self) -> &mut Self::PaintLevel;
}
```

```rust
// legion-core/src/traits/mavlink.rs

pub trait MavlinkBackend: Send + Sync {
    async fn arm(&self) -> Result<(), MavlinkError>;
    async fn disarm(&self) -> Result<(), MavlinkError>;
    async fn takeoff(&self, alt_m: f32) -> Result<(), MavlinkError>;
    async fn goto(&self, wp: Waypoint, speed_m_s: f32) -> Result<(), MavlinkError>;
    async fn follow_path(&self, path: &[Waypoint], speed_m_s: f32) -> Result<(), MavlinkError>;
    async fn return_to_launch(&self) -> Result<(), MavlinkError>;
    async fn land(&self) -> Result<(), MavlinkError>;
    async fn hold(&self) -> Result<(), MavlinkError>;
    async fn emergency_pullback(&self) -> Result<(), MavlinkError>;
    async fn inject_rtk(&self, rtcm: &[u8]) -> Result<(), MavlinkError>;
    /// Command the nozzle servo on Pixhawk AUX5. `true` = pressed
    /// (spray on), `false` = released (spray off). Single entry
    /// point for all spray control — the executor, the safety
    /// loop, and the radio-loss policy all funnel through this.
    async fn set_nozzle(&self, open: bool) -> Result<(), MavlinkError>;
    fn position(&self) -> Position;
    fn battery_pct(&self) -> f32;
}
```

The `async fn` here is *stable* async fn in trait (Rust ≥1.75). It compiles down to a `Future` whose concrete type is determined by the impl — tokio's `RustMavlinkDriver` produces a tokio-friendly future; embassy's `EmbassyMavlinkDriver` produces an embassy-friendly one. The trait definition itself is runtime-agnostic.

**v1 ships only the `legion` (Pi) binary.** `legion-mcu` is a v3 deliverable. The point of the split now is so v3 isn't a fork — it's a sibling crate that re-uses the core.

## Process model

Legion is one Rust binary running ~5 concurrent tokio tasks, sharing state via a single `LegionState` behind an `Arc<RwLock<...>>`.

```
                     ┌──────────────────────────────┐
                     │       LegionState (Arc)      │
                     │  - current sortie            │
                     │  - current step index        │
                     │  - drone phase               │
                     │  - sensor cache              │
                     │  - oracle link state         │
                     │  - last_oracle_contact       │
                     └──────────┬───────────────────┘
                                │
        ┌───────────────────────┼────────────────────────┐
        │                       │                        │
        ▼                       ▼                        ▼
┌─────────────────┐    ┌──────────────────┐    ┌──────────────────┐
│  Comms Client   │    │ Sortie Executor  │    │  Safety Loop     │
│  (Transport)    │    │ (state machine)  │    │ (10 Hz, preempts)│
└────────┬────────┘    └────────┬─────────┘    └────────┬─────────┘
         │                      │                       │
         │                      ▼                       │
         │             ┌──────────────────┐              │
         │             │ Mavlink Driver   │◀─────────────┤
         │             │ (rust-mavlink)   │   safety can │
         │             └────────┬─────────┘   command    │
         │                      │             RTL/LAND   │
         │                      ▼             directly   │
         │             ┌──────────────────┐              │
         │             │  Pixhawk (PX4)   │              │
         │             └──────────────────┘              │
         │                                              │
         ▼                                              ▼
┌─────────────────┐                          ┌──────────────────┐
│Telemetry Pumper │                          │ Payload Drivers  │
│(2 Hz to oracle) │                          │  ToF (rppal I²C) │
└─────────────────┘                          └──────────────────┘
                                             (nozzle is on AUX5,
                                              not on the Pi, see
                                              the Nozzle control
                                              section above)
```

Three things to note:

1. **The Safety Loop preempts the Executor.** If safety fires (ToF too close, paint empty, battery critical, oracle silent past timeout), it interrupts the executor and commands the Pixhawk directly via the MAVLink driver. The executor sees the interruption via state and stops issuing its own commands. Mechanism: a `tokio::sync::watch` channel publishing `SafetyState`, which the executor's step handlers `.await` on alongside their MAVLink futures via `tokio::select!`.
2. **Legion always has the full sortie locally.** The comms client receives `UploadSortie`, validates it, persists to `/var/lib/legion/sorties/<id>.json`, and stores it in `LegionState`. From that moment on, the executor can drive the drone through the sortie even if oracle goes silent — guided by the per-step radio loss policy.
3. **Step transitions wait for `Proceed` from oracle.** This is the explicit gating handshake. The executor finishes step N, sends `StepComplete`, then *blocks* on receipt of `Proceed { sortie_id, expected_step_index: N+1 }` from oracle before starting step N+1. If oracle is silent for the configured per-step timeout, the radio loss policy kicks in.

## Module layout

The brain lives in `legion-core` (no_std, portable). The Pi-specific bindings live in `legion` (std + tokio + rppal).

### `legion-core/` — portable core

```
legion-core/
├── Cargo.toml                   ← #![no_std], depends on hivemind-protocol
└── src/
    ├── lib.rs                   ← #![no_std]; extern crate alloc;
    ├── error.rs                 ← thiserror-no-std error types
    │
    ├── traits/                  ← *definitions only*, no impls
    │   ├── mod.rs
    │   ├── payload.rs           ← Tof, PaintLevel, Payload super-trait (no Pump / Nozzle — nozzle lives on MavlinkBackend)
    │   ├── mavlink.rs           ← MavlinkBackend (async fn in trait)
    │   ├── transport.rs         ← re-export Transport from hivemind-protocol
    │   ├── store.rs             ← SortieStore
    │   └── clock.rs             ← Clock (so the safety loop can ask "how long since X" without std::time)
    │
    ├── executor/                ← sortie execution, generic over the traits
    │   ├── mod.rs
    │   ├── machine.rs           ← Executor<P, M, S, T>: state machine + step iterator + Proceed handshake
    │   ├── steps.rs             ← per-StepType handlers (Takeoff, Transit, SprayPass, Refill, RTL, Land)
    │   └── radio_loss.rs        ← per-step policy enforcement
    │
    ├── safety/                  ← single-tick safety check
    │   ├── mod.rs
    │   ├── check.rs             ← async fn safety_check<P, M, C>(...) -> SafetyOutcome  (no loop!)
    │   └── checks.rs            ← individual checks (ToF, battery, paint, oracle heartbeat)
    │
    └── state.rs                 ← LegionState core data (no Arc/Mutex — those are runtime concerns)
```

### `legion/` — Pi-side runtime (std + tokio)

```
legion/
├── README.md                    ← this file
├── Cargo.toml                   ← workspace member; depends on legion-core + hivemind-protocol
├── systemd/
│   └── legion.service
└── src/
    ├── main.rs                  ← clap entrypoint, dispatches to serve / debug subcommands
    ├── lib.rs                   ← re-exports for integration tests
    │
    ├── config.rs                ← figment-loaded config (TOML + env)
    ├── runtime.rs               ← tokio main loop: hosts the Executor + safety wrapper from legion-core
    │
    ├── comms/                   ← oracle link client wrapping hivemind_protocol::Transport
    │   ├── mod.rs
    │   ├── client.rs            ← opens the Transport, runs the read/write loops
    │   └── reconnect.rs         ← exponential backoff for the serial reopen / TCP reconnect
    │
    ├── mavlink_driver/          ← legion_core::traits::MavlinkBackend impl using rust-mavlink
    │   ├── mod.rs               ← RustMavlinkDriver
    │   ├── connection.rs        ← rust-mavlink + tokio-serial setup, heartbeat wait
    │   ├── flight.rs            ← arm, takeoff, goto, follow_path, RTL, land
    │   ├── telemetry.rs         ← decode HEARTBEAT, GLOBAL_POSITION_INT, BATTERY_STATUS, ATTITUDE, MISSION_CURRENT
    │   └── rtk.rs               ← inject GPS_RTCM_DATA from oracle's RtkCorrection frames
    │
    ├── payload/                 ← legion_core::traits::Payload impls (Pi sensors only)
    │   ├── mod.rs               ← RppalPayload super-impl
    │   ├── tof.rs               ← RppalTof (I²C, VL53L1X on v1)
    │   ├── paint_level.rs       ← NotInstalledPaintLevel on v1; real HX711 impl on v2
    │   └── mock.rs              ← MockPayload (healthy-default sensor readings for SITL/dev)
    │
    ├── store/                   ← legion_core::traits::SortieStore impl: file-backed
    │   ├── mod.rs
    │   └── file_store.rs        ← read/write/list/checkpoint sorties as serde_json files
    │
    ├── safety_loop.rs           ← thin tokio wrapper: tick at 10 Hz, call legion_core::safety::check
    │
    └── cli/
        ├── mod.rs
        ├── serve.rs             ← `legion serve` (the production daemon)
        └── debug.rs             ← `legion debug …` subcommands for hand-testing
```

The layering: every file in `legion/src/` is either Pi-specific glue (clap, figment, tracing, rppal, tokio-serial, std::fs) or a thin runtime wrapper around something from `legion-core`. The MCU port replaces all of these with embassy/embedded-hal/defmt equivalents — but never touches `legion-core`.

## The Sortie

The `Sortie` type is defined in **`hivemind-protocol`**, not in legion's own source. Both binaries import it. Here's the shape — full definitions live in `protocol/src/sortie.rs`:

```rust
// hivemind-protocol/src/sortie.rs

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Sortie {
    pub sortie_id: SortieId,
    pub plan_id: PlanId,
    pub drone_id: DroneId,
    pub steps: Vec<SortieStep>,
    pub paint_volume_ml: f64,
    pub expected_duration: Duration,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SortieStep {
    pub index: u32,                          // 0-based, monotonic within the sortie
    pub step_type: StepType,
    pub waypoint: Waypoint,
    pub path: Option<Vec<Waypoint>>,         // for SPRAY_PASS / TRANSIT segments
    pub speed_m_s: f32,
    pub spray: bool,
    pub radio_loss: RadioLossPolicy,
    pub expected_duration: Duration,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub enum StepType {
    Takeoff,
    Transit,
    SprayPass,
    RefillApproach,
    RefillWait,
    ReturnToBase,
    Land,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RadioLossPolicy {
    pub behaviour: RadioLossBehaviour,
    pub silent_timeout_s: f32,
    pub hold_then_rtl_after_s: Option<f32>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub enum RadioLossBehaviour {
    Continue,
    HoldThenRtl,
    RtlImmediately,
}
```

The key field is `radio_loss` per step. Different step types want different "what to do if oracle dies mid-step" answers. Oracle's slicer fills in defaults; the operator can override per step. Defaults:

| Step type | Default behaviour | Default `silent_timeout_s` | Why |
|---|---|---|---|
| `Takeoff` | `HoldThenRtl` | 5 s | We just left the ground; if oracle hasn't said proceed, hover briefly then RTL. |
| `Transit` | `Continue` | 30 s | Just flying through clear air; finishing the transit and waiting at the destination is fine. |
| `SprayPass` | `Continue` | 60 s | We're on the rails; finishing the pass and reporting completion is the right thing. |
| `RefillApproach` | `HoldThenRtl` | 15 s | Refill station is a hot zone; if oracle is silent, hold a safe distance then RTL. |
| `RefillWait` | `HoldThenRtl` | 60 s | Ground crew needs time. Generous timeout. |
| `ReturnToBase` | `Continue` | 30 s | RTL is always safe. |
| `Land` | `Continue` | 30 s | Definitely just land. |

These are defaults — the slicer can tighten them for any step (e.g. for a spray pass over traffic, force `RtlImmediately`). Legion just enforces what's in the file.

## Sortie execution

The executor lives in **`legion-core/src/executor/`** and is generic over the hardware traits — it doesn't know it's running on a Pi vs an MCU. Pseudocode (simplified):

```rust
pub async fn execute_sortie(
    sortie: Sortie,
    state: Arc<RwLock<LegionState>>,
    link: &mut Link,
    mavlink: &MavlinkDriver,
    payload: &Payload,
    safety_rx: &mut watch::Receiver<SafetyState>,
) -> Result<()> {
    state.write().await.current_sortie = Some(sortie.clone());
    state.write().await.current_step_index = 0;
    sortie_store::persist(&sortie).await?;

    link.send(LegionToOracle::SortieReceived { sortie_id: sortie.sortie_id }).await?;

    for step in &sortie.steps {
        state.write().await.current_step_index = step.index;

        // 1. Wait for explicit Proceed from oracle (or trip the radio loss policy)
        let proceed = link.wait_for_proceed(
            sortie.sortie_id,
            step.index,
            Duration::from_secs_f32(step.radio_loss.silent_timeout_s),
        );

        match proceed.await {
            Ok(_) => {}
            Err(WaitError::Timeout) => {
                radio_loss::apply(step, &sortie, &state, mavlink, link).await?;
                return Ok(());
            }
            Err(e) => return Err(e.into()),
        }

        // 2. Run the step (delegate to a per-StepType handler)
        // The handler runs concurrently with the safety watcher; whichever finishes first wins.
        let result = tokio::select! {
            r = steps::run(step, &state, mavlink, payload) => r?,
            preempt = safety_rx.changed() => {
                preempt?;
                let safety = safety_rx.borrow().clone();
                link.send(LegionToOracle::SortieFailed {
                    sortie_id: sortie.sortie_id,
                    step_index: step.index,
                    reason: format!("safety preemption: {safety:?}"),
                }).await?;
                return Ok(());
            }
        };

        // 3. Tell oracle we finished the step
        let snap = state.read().await;
        link.send(LegionToOracle::StepComplete {
            sortie_id: sortie.sortie_id,
            step_index: step.index,
            position: snap.position,
            battery_pct: snap.battery_pct,
            paint_remaining_ml: snap.paint_remaining_ml,
            duration_s: result.duration.as_secs_f32(),
        }).await?;

        sortie_store::checkpoint(&sortie.sortie_id, step.index).await?;
    }

    link.send(LegionToOracle::SortieComplete { sortie_id: sortie.sortie_id }).await?;
    Ok(())
}
```

Invariants:

- **Proceed must be explicit.** Even after a clean step completion, the executor blocks until oracle says go. This is what makes oracle the gate for every transition — operator approval lives at the *plan* level, oracle's deconfliction logic lives at the *step* level.
- **The radio loss timeout is per step.** A short transit step can have a 5 s timeout; a multi-minute spray pass can have a 60 s timeout. The slicer picks them; legion enforces them.
- **Safety preemption beats everything.** The `tokio::select!` between step execution and `safety_rx.changed()` means the executor never holds the bus when safety needs it.
- **`expected_step_index` is checked against the current step.** If oracle sends `Proceed { expected_step_index: 4 }` while legion is on step 2 (because of a duplicate or out-of-order frame), legion rejects it and reports an `Error`. No accidental skipping.

### Per-step radio loss policy

```rust
pub async fn apply(
    step: &SortieStep,
    sortie: &Sortie,
    state: &Arc<RwLock<LegionState>>,
    mavlink: &MavlinkDriver,
    link: &mut Link,
) -> Result<()> {
    match step.radio_loss.behaviour {
        RadioLossBehaviour::Continue => {
            // Finish the step autonomously, then hover at destination.
            let _ = steps::run(step, state, mavlink, /* payload */).await;
            mavlink.hold().await?;
        }
        RadioLossBehaviour::HoldThenRtl => {
            mavlink.hold().await?;
            let after = step.radio_loss.hold_then_rtl_after_s.unwrap_or(30.0);
            tokio::time::sleep(Duration::from_secs_f32(after)).await;
            if !link.is_connected() {
                mavlink.return_to_launch().await?;
            }
        }
        RadioLossBehaviour::RtlImmediately => {
            mavlink.return_to_launch().await?;
        }
    }
    Ok(())
}
```

The split between *the safety loop* and *the radio loss policy* is intentional:

- The **safety loop** is always-on and drone-only. It doesn't know about sorties or steps. It reads sensors and reacts to physical danger.
- The **radio loss policy** is per step and context-aware. It knows what the drone is doing and what the operator decided was safe to continue without supervision.

When they overlap (oracle goes silent), the safety loop stops the *spray* immediately — preventing wasted paint and the wrong-spot risk — and the radio loss policy decides the *flight* outcome. They are not redundant; they are layered.

## The wire protocol

Defined in **`hivemind-protocol`**. Both ends import the same types. Message catalogue:

**Oracle → legion** (commands):

| Variant | Fields | Notes |
|---|---|---|
| `Hello` | `oracle_version, server_time` | First frame after the transport opens. |
| `Heartbeat` | `ts` | 2 Hz. Resets legion's `oracle_silent` watchdog. |
| `UploadSortie` | `sortie: Sortie` | Full Sortie. Legion validates, persists, then sends `SortieReceived`. |
| `Proceed` | `sortie_id, expected_step_index` | Unblocks the executor for the next step. |
| `HoldStep` | `sortie_id, reason` | Tell legion to hold at the current position before starting the next step. |
| `AbortSortie` | `sortie_id, reason` | Clean abort; legion stops the executor, RTLs the drone, reports `SortieFailed`. |
| `ReturnToBase` | `reason` | Hard RTL — overrides whatever step is in flight. |
| `RtkCorrection` | `payload: Vec<u8>` | Opaque RTCM3 bytes. Legion injects via `mavlink::rtk`. |
| `CancelSortie` | `sortie_id` | Drop a sortie that hasn't started. Errors if it's already executing. |

**Legion → oracle** (status):

| Variant | Fields | Notes |
|---|---|---|
| `Hello` | `drone_id, legion_version, capabilities, in_progress_sortie?` | First frame. `in_progress_sortie` is non-null if legion booted with a partially-completed sortie on disk. |
| `Heartbeat` | `ts` | 2 Hz. |
| `Telemetry` | `position, attitude, battery_pct, voltage, paint_remaining_ml, tof_distance_cm, gps_fix, sortie_id?, step_index?, drone_phase` | 2 Hz. The single source of truth for fleet state. |
| `SortieReceived` | `sortie_id` | Validation passed, persisted, ready to execute. |
| `StepComplete` | `sortie_id, step_index, position, battery_pct, paint_remaining_ml, duration_s` | Sent after the step handler returns successfully. Executor then blocks waiting for `Proceed`. |
| `SortieComplete` | `sortie_id` | All steps done. |
| `SortieFailed` | `sortie_id, step_index, reason` | Clean abort or unrecoverable executor error. |
| `SafetyEvent` | `kind, action, detail` | One of `tof_avoidance \| battery_critical \| paint_empty \| oracle_silent`. |
| `Held` | `sortie_id, step_index, reason` | Confirmation that legion is holding (whether from `HoldStep` or from a radio-loss policy). |
| `Error` | `code, message` | Out-of-protocol issues (bad frame, version mismatch, expected_step_index out of order). |

### Wire format and transport

**Wire format:** [postcard](https://postcard.jamesmunns.com/) is the de-facto compact binary format for Rust serde types. Designed for embedded; small overhead; deterministic. Schema versioning is handled by including a `version: u8` in the `Hello` exchange and refusing mismatched versions.

**Framing:** [COBS](https://en.wikipedia.org/wiki/Consistent_Overhead_Byte_Stuffing) (Consistent Overhead Byte Stuffing). Adds at most 1 byte per 254 bytes of payload. Critically, COBS uses `0x00` as a frame delimiter and guarantees that delimiter never appears mid-frame, so a serial receiver can re-synchronise after byte loss without losing the rest of the stream.

Combined: every message is `cobs(postcard(message)) || 0x00`. A typical telemetry frame is ~80 bytes; a typical sortie upload is ~2–5 kB.

**Transport trait:**

```rust
// hivemind-protocol/src/transport.rs

#[async_trait]
pub trait Transport: Send {
    async fn send(&mut self, msg: &OracleToLegion) -> Result<(), TransportError>;
    async fn recv(&mut self) -> Result<LegionToOracle, TransportError>;
}

// In legion the directions are flipped — legion implements the client side
// where send is LegionToOracle and recv is OracleToLegion. The crate provides
// generic helpers parameterised by the message type for both directions.
```

Two concrete impls in the protocol crate, behind cargo features:

- **`SerialTransport`** (feature = `serial`, used in v1 production): wraps `tokio_serial::SerialStream`. Reads bytes, runs them through a COBS decoder, dispatches complete frames to postcard.
- **`TcpTransport`** (feature = `tcp`, used in SITL/dev/v2 IP radios): wraps `tokio::net::TcpStream`. Same COBS-postcard framing — TCP gives reliability but we still use the same bytes so the protocol stays transport-agnostic.

Heartbeat policy:

- **Oracle → legion: 2 Hz.** Legion's safety loop watches the gap and triggers `oracle_silent` after 5 s of nothing. The behaviour after that is governed by the *current step's* radio loss policy.
- **Legion → oracle: 2 Hz**, piggy-backed on the `Telemetry` message stream. Oracle marks the drone `Stale` if it misses N in a row.
- **Connection drop ≠ drone failure.** Legion keeps executing whatever the current step's policy says to do, and reconnects with backoff (or, for serial, just keeps the port open and waits for the next frame).

## The local safety loop

The safety *check* lives in `legion-core/src/safety/check.rs` as a single `async fn` over the hardware traits. The 10 Hz *loop* — the thing that calls `tokio::time::interval` or `embassy::time::Timer` — lives in the binary that hosts `legion-core`. This is the runtime split: timing primitives are not portable, the logic is.

The Pi-side wrapper in `legion/src/safety_loop.rs`:

```rust
pub async fn safety_loop<P, M, C>(
    state: Arc<RwLock<LegionState>>,
    mut payload: P,
    mavlink: Arc<M>,
    clock: C,
    link: Arc<Mutex<Link>>,
    safety_tx: watch::Sender<SafetyState>,
    cfg: SafetyConfig,
) where
    P: Payload + Send,
    M: MavlinkBackend + Send + Sync,
    C: Clock + Send,
{
    let mut tick = tokio::time::interval(Duration::from_millis(100)); // 10 Hz
    loop {
        tick.tick().await;

        // 1. ToF wall avoidance
        if let Ok(tof) = payload.tof().read_cm().await {
            if tof < cfg.tof_min_cm {
                emit(&safety_tx, SafetyState::TofAvoidance { tof_cm: tof });
                let _ = mavlink.set_nozzle(false).await;     // cut the nozzle (AUX5)
                let _ = mavlink.emergency_pullback().await;
                continue;
            }
        }

        // 2. Battery critical
        let battery = mavlink.battery_pct();
        if battery > 0.0 && battery < cfg.battery_critical_pct {
            emit(&safety_tx, SafetyState::BatteryCritical { battery_pct: battery });
            let _ = mavlink.set_nozzle(false).await;
            let _ = mavlink.return_to_launch().await;
            continue;
        }

        // 3. Paint empty (v1: skipped — paint_level returns NotInstalled,
        //    see hw/nozzle/README.md; v2 load cell will populate this).
        if let Ok(paint) = payload.paint_level().read_ml().await {
            if paint < cfg.paint_empty_ml {
                emit(&safety_tx, SafetyState::PaintEmpty { paint_ml: paint });
                let _ = mavlink.set_nozzle(false).await;
                let _ = mavlink.return_to_launch().await;
                continue;
            }
        }

        // 4. Oracle heartbeat — only stop spraying. The executor's radio
        //    loss policy decides what to do about flight.
        let silence = state.read().await.last_oracle_contact_ms;
        if clock.elapsed_ms(silence) > cfg.oracle_silent_ms {
            let _ = mavlink.set_nozzle(false).await;
        }
    }
}
```

The body of the loop dispatches to `legion_core::safety::check(...)` (single iteration, no allocs, no timer types). Replacing the wrapper with embassy's `Timer::after(...).await` is the entire MCU-side change for the safety subsystem.

## MAVLink driver

A `legion_core::traits::MavlinkBackend` impl using `rust-mavlink` + `tokio-serial`. The trait lives in `legion-core`; the impl lives in `legion/src/mavlink_driver/`. The Pi binary is the only place that imports `mavlink::*` — the core sees only the trait. Sketch of the impl:

```rust
pub struct MavlinkDriver {
    conn: Arc<Mutex<Box<dyn MavConnection<MavMessage> + Send + Sync>>>,
    sysid: u8,
    compid: u8,
}

impl MavlinkDriver {
    pub async fn connect(address: &str) -> Result<Self> {
        // address: "serial:/dev/ttyAMA0:921600"
        let conn = mavlink::connect::<MavMessage>(address)?;
        let driver = Self { conn: Arc::new(Mutex::new(conn)), sysid: 1, compid: 1 };
        driver.wait_for_heartbeat().await?;
        Ok(driver)
    }

    pub async fn arm(&self) -> Result<()> {
        self.send_command_long(MAV_CMD::MAV_CMD_COMPONENT_ARM_DISARM, [1.0, 0., 0., 0., 0., 0., 0.]).await
    }

    pub async fn takeoff(&self, alt_m: f32) -> Result<()> {
        self.send_command_long(MAV_CMD::MAV_CMD_NAV_TAKEOFF, [0., 0., 0., f32::NAN, f32::NAN, f32::NAN, alt_m]).await?;
        self.wait_for_altitude(alt_m, 0.5).await
    }

    pub async fn goto(&self, wp: &Waypoint, speed_m_s: f32) -> Result<()> {
        self.set_max_speed(speed_m_s).await?;
        self.send_set_position_target_global_int(wp).await?;
        self.wait_for_position(wp, 0.5).await
    }

    pub async fn follow_path(&self, path: &[Waypoint], speed_m_s: f32) -> Result<()> {
        self.enter_offboard_mode().await?;
        for wp in path {
            self.send_set_position_target_local_ned(wp).await?;
            self.wait_for_position(wp, 0.3).await?;
        }
        self.exit_offboard_mode().await
    }

    pub async fn return_to_launch(&self) -> Result<()> {
        self.send_command_long(MAV_CMD::MAV_CMD_NAV_RETURN_TO_LAUNCH, [0.; 7]).await?;
        self.wait_for_landed().await
    }

    pub async fn land(&self) -> Result<()> {
        self.send_command_long(MAV_CMD::MAV_CMD_NAV_LAND, [0.; 7]).await?;
        self.wait_for_landed().await
    }

    pub async fn hold(&self) -> Result<()> {
        self.set_mode(PX4_CUSTOM_MAIN_MODE_AUTO, PX4_CUSTOM_SUB_MODE_AUTO_LOITER).await
    }

    pub async fn emergency_pullback(&self) -> Result<()> {
        // Velocity setpoint pointing back from the obstacle, then HOLD.
        self.send_set_velocity_body([-0.5, 0.0, 0.0]).await?;
        tokio::time::sleep(Duration::from_secs(1)).await;
        self.hold().await
    }

    pub async fn inject_rtk(&self, rtcm: &[u8]) -> Result<()> {
        // Fragment into GPS_RTCM_DATA (max 180 bytes per message)
        for fragment in rtcm.chunks(180) {
            self.send_gps_rtcm_data(fragment).await?;
        }
        Ok(())
    }
}
```

This module is the **only** code in `legion` that imports from `mavlink::*`. Everything else (the executor, the safety loop, the radio-loss policy) deals with the `MavlinkBackend` trait from `legion-core`, which makes both the executor unit-testable against a stub *and* the future MCU port a drop-in (the embassy version implements the same trait).

The Rust hand-rolled wrapper is a few hundred lines — bigger than the MAVSDK-Python equivalent, but: smaller binary, no gRPC, no C++ runtime, and we control every retry/timeout decision. It also compiles cleanly against the `mavlink` crate's `no_std` mode, which the future `legion-mcu` port will exercise — though the `tokio-serial` part is replaced by `embassy-stm32-usart` (or similar) on that side.

## Persistence

`legion-core` defines a `SortieStore` trait. The Pi binary's `FileSortieStore` impl puts sorties in `/var/lib/legion/sorties/<sortie_id>.json` as `serde_json` files. Each completed step is checkpointed by writing a sibling `<sortie_id>.progress.json` containing `{ last_completed_step, ts, ... }`. Atomic via `tempfile::NamedTempFile` + `persist`. The MCU port will replace this with a flash-backed impl (or a `NullStore` that simply forgets between reboots — fine if the MCU's reaction to a power cycle is "report `Hello` with no in-progress sortie and wait for instructions").

On boot, legion scans the directory:

- If no sortie is in progress, legion comes up idle, opens the transport, and waits.
- If a sortie is in progress (progress file present, `SortieComplete` not yet sent), legion does **not** automatically resume it. It includes `in_progress_sortie: Some(InProgressSortie { sortie_id, last_completed_step })` in its `Hello` and waits for explicit instructions. The decision of whether to resume, abort, or restart is the operator's, made through pantheon.

This is the same conservative posture the parent oracle README takes for plan recovery: state survives crashes, but execution does not auto-restart.

## Boot sequence

1. systemd starts `legion.service`.
2. legion loads `/etc/legion/config.toml`.
3. Connects MAVLink via `tokio-serial` to `/dev/ttyAMA0` (Pixhawk TELEM2). Waits for the first `HEARTBEAT`. Times out after 30 s and exits.
4. Initialises the Pi-side payload drivers via `rppal` (VL53L1X ToF over I²C). Issues `mavlink.set_nozzle(false)` to leave the Pixhawk AUX5 servo released before anything else touches it.
5. **Starts the safety loop.** From this moment forward, the drone is protected.
6. Opens the radio transport (`SerialTransport` on `/dev/ttyUSB0`). Sends `Hello { drone_id, capabilities, in_progress_sortie? }`. Reads the persistence directory and reports any in-progress sortie.
7. Idle, waiting for `UploadSortie`.

If MAVLink fails to connect within the timeout, legion exits with a nonzero code. systemd restarts it. The drone is intentionally inert until legion is up — there is no "fly without legion" mode.

## Configuration

`/etc/legion/config.toml`:

```toml
[drone]
id = "drone-01"
capabilities = ["spray", "rtk", "tof"]

[mavlink]
address = "serial:/dev/ttyAMA0:921600"   # Pixhawk TELEM2 UART
connect_timeout_s = 30

[transport]
# v1: serial radio. Swap to "tcp" for SITL/dev.
kind = "serial"
serial_path = "/dev/ttyUSB0"
serial_baud = 57600
# Used only when kind = "tcp"
tcp_addr = "10.42.0.1:7346"

[oracle]
shared_token = "${LEGION_ORACLE_TOKEN}"
heartbeat_hz = 2
reconnect_initial_s = 1.0
reconnect_max_s = 30.0

[safety]
tof_min_cm = 30
battery_critical_pct = 15
paint_empty_ml = 20
oracle_silent_s = 5.0

[storage]
sortie_dir = "/var/lib/legion/sorties"

[payload]
# Forward ToF sensor — VL53L1X on the Pi's I²C bus. The ToF is the
# only Pi-side peripheral legion drives directly on v1.
tof_i2c_bus = 1
tof_i2c_address = 0x29

[nozzle]
# v1 spray servo lives on Pixhawk AUX5, not on the Pi — see
# hw/nozzle/README.md. legion commands it via MAVLink
# (MAV_CMD_DO_SET_SERVO). Only the servo index + PWM endpoints
# are configurable; the physical wiring is fixed to AUX5.
aux_servo_index = 5
pwm_open_us  = 2000    # matches PWM_AUX_MAX5 in PX4 params
pwm_closed_us = 1000   # matches PWM_AUX_MIN5 in PX4 params
```

## CLI

Legion ships a clap-driven CLI with `serve` (the production daemon entrypoint) plus a `debug` subcommand tree for hand-testing without oracle:

```
$ legion serve                                    # production daemon (what systemd runs)
$ legion debug status                             # show drone state, sensor reads, transport state
$ legion debug arm                                # arm the autopilot (refuses if not in safe ground state)
$ legion debug fly-to <lat> <lon> <alt>           # send a single goto
$ legion debug nozzle on|off                      # toggle the AUX5 spray servo via MAVLink
$ legion debug load-sortie <file.json>            # bypass oracle, load a sortie locally
$ legion debug execute --auto-proceed             # run the loaded sortie autonomously
$ legion debug protocol-tail                      # pretty-print every frame on the transport
```

Both `serve` and `debug` import the same modules — no parallel implementations. Authentication on the debug CLI is just "you're on the Pi already, you're trusted."

## Testing strategy

1. **Unit tests** for the executor state machine. Run the executor against a `MockMavlinkDriver` and a `MockTransport` that scripts a `Proceed`/`HoldStep`/`AbortSortie` sequence. Validates: every step completes correctly, every radio-loss policy triggers correctly, safety preemption interrupts cleanly, `expected_step_index` mismatches are rejected.
2. **Protocol round-trip tests** in the `hivemind-protocol` crate that take every message type, postcard-encode/COBS-frame/decode it, and assert deep equality.
3. **Hardware mock layer.** `Tof` and `PaintLevel` are the Pi-side payload traits; v1 has `RppalTof` (VL53L1X over I²C) and a `NotInstalledPaintLevel` stub. Tests run against `MockPayload`; production runs against rppal. The nozzle has no trait in `legion-core::traits::payload` — it's on `MavlinkBackend::set_nozzle`, backed by `StubMavlinkDriver` in tests and `RustMavlinkDriver` in production.
4. **PX4 SITL gate** (nightly). Real legion process, real `mavlink` crate, against PX4 SITL in docker. Loads a fixture sortie via the debug CLI, executes it without oracle, asserts the SITL drone reaches the planned waypoints.
5. **Oracle pair test** (nightly). Real legion + real oracle (both Rust binaries from the same workspace), with PX4 SITL on the legion side and a `TcpTransport` between the two binaries (so we don't need a real radio in CI). Full end-to-end: oracle uploads sortie, legion confirms steps, sortie completes.

## What v1 ships

- `legion-core` crate (no_std + alloc) with the executor, safety check, radio-loss policy, and hardware traits — generic over backends, ready to host on either Pi or a future MCU.
- `legion` Rust binary, runnable via `legion serve` or as a systemd service. Pi 5-only.
- `RustMavlinkDriver` impl covering arm, takeoff, goto, follow_path (offboard), RTL, land, hold, RTK injection.
- Sortie executor with the full step-confirmation handshake and per-step radio loss policy.
- 10 Hz safety loop wrapper around `legion_core::safety::check`.
- `RppalPayload` impl for the v1 hardware — forward VL53L1X over I²C. No Pi-side pump, no Pi-side nozzle, no paint-level ADC: the spray servo is commanded via MAVLink to Pixhawk AUX5 (see [hw/nozzle](../../hw/nozzle/README.md)), and v1 has no load cell.
- `FileSortieStore` impl: sorties + checkpointed progress as serde_json files in `/var/lib/legion/`.
- `SerialTransport` over `tokio-serial` for the v1 radio (HolyBro SiK 433 MHz EU variant; RFD868x for v2, same UART interface).
- `TcpTransport` (behind `--features tcp`) for SITL and dev.
- Debug CLI for hand-testing.
- systemd service unit.

## What v1 does *not* ship

- **Auto-resume of interrupted sorties.** Legion reports the in-progress sortie in its `Hello`; the operator decides via pantheon.
- **Multiple-drone awareness.** Legion knows about exactly one drone (its own) by design — that's the parent README's star topology.
- **Local mission planning.** Legion never generates paths. If a sortie isn't in `LegionState`, the drone doesn't fly.
- **Independent QGroundControl backup channel.** The hardware *supports* it (TELEM1 is free), but plumbing it is a hardware decision tracked in [hw/](../hw/README.md), not legion.
- **Vision/camera processing.** Pi 5 has the headroom but v1 has nothing visual to do.
- **`mavsdk` Rust crate (gRPC to mavsdk-server).** v1 uses `rust-mavlink` directly. We re-evaluate if MAVLink mission-upload semantics turn out hairier than worth hand-rolling.
- **`legion-mcu` (the bare-metal port).** v3, not v1 or v2. The point of the `legion-core` split *now* is so v3 isn't a fork — but no v1 effort goes into the MCU side. v1 is Pi-only.
- **915 MHz hardware.** Illegal to operate in the EU. The whole project commits to 433 MHz (v1 cheap) or 868 MHz EU SRD-d (v2 production) telemetry radios.

## Open questions

1. **PX4 mission-mode vs offboard for spray paths.** v1 sketches use `set_position_target_local_ned` in offboard mode for spray-pass paths. The alternative is uploading a short MAVLink mission per pass and letting PX4 fly it autonomously — slower handshake, more robust against the link blip. Decision tied to the SITL spike.
2. **ToF sensor choice.** v1 leans VL53L1X (4 m range, I²C, cheap, well-supported by `rppal`). For larger drones with longer standoff requirements, a TFmini Plus (12 m, UART) is the upgrade. Tracked in [hw/v1](../hw/v1/README.md).
3. **Yaw control during spray passes.** Spraying needs the nozzle pointed at the surface — that's a yaw command. Open: do we lock yaw during a `SprayPass` step (operator picks at slice time), or compute it from the surface normal in the path waypoints? Defaults to "lock yaw to `step.waypoint.yaw_deg`" for v1.
4. **Backpressure when oracle floods commands.** What does legion do if oracle, during a chaotic moment, sends `HoldStep` then `Proceed` then `AbortSortie` in quick succession? v1 processes them in receive order (single tokio task pulling from the transport). Worth thinking about whether `AbortSortie` should preempt earlier queued messages.
5. **Global silence failsafe.** After `RadioLossBehaviour::Continue` finishes the last step, legion ends up holding at the destination indefinitely until oracle reconnects or another policy fires. Should there be a `[safety] absolute_silence_rtl_after_s` failsafe (e.g. RTL after 5 minutes of total silence regardless of step policy)? Probably yes.
6. **Workspace lints + MSRV.** New workspace, new chance to set lints (`clippy::pedantic`?), MSRV (1.75? 1.80?), and `[workspace.dependencies]` for shared deps. Decision deferred to the Cargo.toml-creation pass, but worth flagging now so we don't drift between oracle and legion.
7. **Concrete MCU target for v3.** Realistic candidates: STM32H7 (256+ KB RAM, 1 MB+ flash, used by Pixhawk itself), RP2040 (Pi Pico, cheaper but no FPU), nRF9160/nRF52 (good for power-constrained but may be tight). Decision not needed for v1, but worth keeping the no_std assumptions of `legion-core` validated against at least one realistic target so it doesn't drift into "Pi-only that *claims* to be portable."
8. **Async fn in trait + dyn dispatch.** Stable async fn in trait makes the traits *non-object-safe* by default, which means we can't have `dyn MavlinkBackend`. Concrete generics work fine for v1 (we have one impl). If we ever want runtime impl swapping (e.g. mock vs real, with config selecting which), we may need `async-trait` (heap-allocates Box<dyn Future> per call) or `embedded-hal-async`-style explicit `Future` associated types. Probably fine to defer until we hit it.
9. **Duty cycle compliance on the v1 433 MHz radio.** EU 433.05–434.79 MHz allows 10 mW EIRP at 10% duty cycle. Steady-state telemetry at 2 Hz on a single drone is well within that, but worth double-checking against the SiK firmware's listen-before-talk behaviour and the actual airtime per `Telemetry` frame. v2 RFD868x in the SRD-d sub-band (500 mW, 10% duty cycle) is much more comfortable.
