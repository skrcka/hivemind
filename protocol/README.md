# hivemind-protocol

The wire-format crate. Defines the message types and the framing on which oracle and legion communicate, plus the `Transport` trait abstraction that lets the same bytes flow over a serial UART (production EU radios) or a TCP socket (SITL, dev, future IP radios).

> See the top-level [README](../README.md) for project context. The two consumers are [oracle](../oracle/README.md) (truck-side, `std + tokio`) and [legion](../legion/README.md) (drone-side; today `std + tokio` on Pi 5, tomorrow `no_std + embassy` on a Cortex-M).

## Why this exists

Oracle and legion are two separate Rust binaries on opposite ends of a radio link. Either:

(a) they each define their own copy of every message type, and we hand-keep the definitions in sync — which guarantees wire-format drift; or

(b) they both depend on a single shared crate that owns the type definitions, the codec, and the transport abstraction — which makes wire-format drift a `cargo check` error.

We do (b). The crate is **`hivemind-protocol`**.

## What's in it

- **Message enums:** `OracleToLegion` (commands) and `LegionToOracle` (status + telemetry).
- **Domain types** that appear in those messages: `Sortie`, `SortieStep`, `RadioLossPolicy`, `StepType`, `Waypoint`, `Position`, `Attitude`, `BatteryState`, `GpsFixType`, `DronePhase`, etc.
- **Wire codec:** [postcard](https://postcard.jamesmunns.com/) (compact serde-driven binary) for serialization, [COBS](https://en.wikipedia.org/wiki/Consistent_Overhead_Byte_Stuffing) for self-synchronizing framing.
- **`Transport` trait** with concrete `TcpTransport` and `SerialTransport` impls behind feature flags.
- **Version negotiation** via the `Hello` exchange.

Nothing else lives here — no business logic, no persistence, no driver code. The goal is "minimum surface area shared between two binaries that need to agree on bytes."

## Crate properties

- **`#![no_std]` with `extern crate alloc;`.** The `alloc` requirement is real — message bodies use `Vec`, `String`, etc. Fully no-alloc would mean `heapless::Vec` everywhere with bounded sizes for everything; we trade that for the ergonomics of standard collection types because the eventual MCU target has enough RAM (≥256 KB) to host a small global allocator.
- **No `std` features required for the type definitions or the codec.** Both compile cleanly under `no_std + alloc`.
- **The `Transport` impls are behind feature flags** that opt into `std`. `feature = "tcp"` brings `tokio::net::TcpStream`; `feature = "serial"` brings `tokio_serial::SerialStream`. Default features are empty so a no_std consumer (like `legion-core`) can depend on the crate without dragging in tokio.

## Workspace position

```
hivemind/
├── Cargo.toml                   ← workspace root
├── protocol/                    ← THIS CRATE  (no_std + alloc)
├── legion-core/                 ← consumes protocol; no_std
├── oracle/                      ← consumes protocol; std + tokio + features = ["serial", "tcp"]
├── legion/                      ← consumes protocol + legion-core; std + tokio + features = ["serial", "tcp"]
└── (future) legion-mcu/         ← consumes protocol + legion-core; no_std + embassy
```

`legion-mcu` will provide its own `EmbassyTransport` impl that lives outside this crate (because it depends on `embassy-stm32` or similar, which we don't want as an optional feature here). The trait definition is the contract; concrete impls can live wherever.

## Message catalogue

This is the canonical reference. Both consumers' READMEs link here rather than re-stating the catalogue.

### Outer envelope

```rust
pub const PROTOCOL_VERSION: u8 = 1;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Envelope<T> {
    pub v: u8,            // protocol version, equals PROTOCOL_VERSION
    pub ts_ms: u64,       // sender's monotonic ms since boot, for jitter analysis
    pub drone_id: DroneId, // routing key on a shared radio channel
    pub msg: T,
}
```

Every frame on the wire is `Envelope<OracleToLegion>` or `Envelope<LegionToOracle>`. The `drone_id` is what lets multiple drones coexist on a single broadcast SiK radio channel — every receiver decodes every frame and ignores ones not addressed to it.

### Oracle → legion (`OracleToLegion`)

| Variant | Fields | Purpose |
|---|---|---|
| `Hello` | `oracle_version: String, server_time_ms: u64` | First frame after the transport opens. Establishes protocol version. |
| `Heartbeat` | — | 2 Hz keepalive. Resets legion's `oracle_silent` watchdog. |
| `UploadSortie` | `sortie: Sortie` | Full Sortie payload. Legion validates, persists, then replies `SortieReceived`. |
| `Proceed` | `sortie_id: SortieId, expected_step_index: u32` | Unblocks legion's executor for the next step. The `expected_step_index` field is checked against legion's current step — out-of-order frames are rejected. |
| `HoldStep` | `sortie_id: SortieId, reason: String` | Tell legion to hold at the current position before starting the next step. |
| `AbortSortie` | `sortie_id: SortieId, reason: String` | Clean abort: legion stops the executor, RTLs the drone, replies `SortieFailed`. |
| `ReturnToBase` | `reason: String` | Hard RTL — overrides whatever step is in flight. |
| `CancelSortie` | `sortie_id: SortieId` | Drop a sortie that hasn't started executing yet. |
| `RtkCorrection` | `payload: Vec<u8>` | Opaque RTCM3 bytes. Legion writes them to the Pixhawk via `mavlink::rtk`. |

### Legion → oracle (`LegionToOracle`)

| Variant | Fields | Purpose |
|---|---|---|
| `Hello` | `drone_id: DroneId, legion_version: String, capabilities: Vec<String>, in_progress_sortie: Option<InProgressSortie>` | First frame from legion. The `in_progress_sortie` field is non-null if legion booted with a partially-completed sortie on disk. |
| `Heartbeat` | — | 2 Hz keepalive (typically piggybacked on `Telemetry`). |
| `Telemetry` | `position, attitude, battery_pct, voltage, paint_remaining_ml, tof_distance_cm, gps_fix, sortie_id?, step_index?, drone_phase` | 2 Hz. The single source of truth for fleet state. |
| `SortieReceived` | `sortie_id: SortieId` | Validation passed, persisted, ready to execute. |
| `StepComplete` | `sortie_id, step_index, position, battery_pct, paint_remaining_ml, duration_s` | Sent after a step handler returns successfully. Executor then blocks waiting for `Proceed`. |
| `SortieComplete` | `sortie_id: SortieId` | All steps done. |
| `SortieFailed` | `sortie_id, step_index, reason` | Clean abort or unrecoverable executor error. |
| `SafetyEvent` | `kind, action, detail` | One of `tof_avoidance \| battery_critical \| paint_empty \| oracle_silent`. Sent every time the safety loop fires. |
| `Held` | `sortie_id, step_index, reason` | Confirmation that legion is holding (whether from `HoldStep` or from a radio-loss policy). |
| `Error` | `code: String, message: String` | Out-of-protocol issues (bad frame, version mismatch, expected_step_index out of order). |

### Sortie / SortieStep / RadioLossPolicy

The Sortie is the unit of work oracle uploads to legion in one frame, then advances step by step.

```rust
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Sortie {
    pub sortie_id: SortieId,
    pub plan_id: PlanId,
    pub drone_id: DroneId,
    pub steps: Vec<SortieStep>,
    pub paint_volume_ml: f32,
    pub expected_duration_s: u32,
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
    pub expected_duration_s: u32,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
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

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum RadioLossBehaviour {
    Continue,
    HoldThenRtl,
    RtlImmediately,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub struct Waypoint {
    pub lat: f64,
    pub lon: f64,
    pub alt_m: f32,
    pub yaw_deg: Option<f32>,
}
```

### Telemetry sub-types

```rust
#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub struct Position { pub lat: f64, pub lon: f64, pub alt_m: f32 }

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub struct Attitude { pub roll_deg: f32, pub pitch_deg: f32, pub yaw_deg: f32 }

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub enum GpsFixType { None, Fix2d, Fix3d, RtkFloat, RtkFixed }

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub enum DronePhase { Idle, Armed, InAir, ExecutingStep, Holding, Landing }
```

## Wire format

Every frame on the wire is:

```
| COBS-encoded postcard bytes | 0x00 |
└─── one frame ──────────────┘└end──┘
```

### Why postcard

- **Compact.** Designed for embedded use; field tags are minimal, integers are varint-encoded. A typical `Telemetry` frame is ~80 bytes; a typical `UploadSortie` is ~2–5 kB depending on step count and path length.
- **Deterministic.** Same input → same bytes. Property-based round-trip tests are trivial.
- **Serde-driven.** Uses the same `#[derive(Serialize, Deserialize)]` everything else uses. No separate IDL.
- **`no_std + alloc` clean.** Compiles for the future MCU target without modification.

### Why COBS

The radios at v1 are serial UARTs (HolyBro SiK 433 MHz at v1, RFD868x at v2 — see [legion's topology section](../legion/README.md#topology)). Serial gives us a byte stream with no inherent framing. We need three things from a framing layer:

1. **Self-synchronization.** A single corrupted byte should lose at most one frame, not desynchronize the receiver permanently.
2. **No mid-frame collision with the delimiter.** The receiver scans for `0x00` to find frame boundaries; the encoded body must never contain `0x00`.
3. **Bounded overhead.** Adding a 5% framing tax on every byte would be unacceptable on a 57600 bps radio.

COBS gives us all three: it transforms an arbitrary byte sequence into one with no `0x00` bytes inside, adding at most 1 byte per 254 bytes of payload (~0.4% overhead). The trailing `0x00` after each encoded frame is the unambiguous delimiter.

The same framing works on TCP — TCP doesn't *need* COBS (it's already a reliable byte stream) but using the same codec on both transports means the protocol crate is fully transport-agnostic at the byte level.

### Worked example

`Heartbeat` from oracle:

```
postcard:  0x01 0x00 0x00 0x00 ...     (Envelope { v: 1, ts_ms: 0, drone_id: "drone-01", msg: Heartbeat })
COBS:      0x05 0x01 0x01 0x01 0x01 ... (no 0x00 bytes inside)
on wire:   0x05 0x01 0x01 0x01 0x01 ... 0x00
                                          ^ frame delimiter
```

The receiver reads bytes until `0x00`, COBS-decodes, postcard-decodes, and gets back an `Envelope<OracleToLegion>` with the original `Heartbeat`.

## The `Transport` trait

```rust
#[trait_variant::make(Transport: Send)]
pub trait LocalTransport {
    type Error: core::fmt::Debug;

    /// Send a single frame. Blocks until the frame is fully written and flushed.
    async fn send<M: Serialize + Send>(&mut self, msg: &Envelope<M>) -> Result<(), Self::Error>;

    /// Receive a single frame. Blocks until a complete frame is decoded.
    async fn recv<M: for<'de> Deserialize<'de> + Send>(&mut self) -> Result<Envelope<M>, Self::Error>;
}
```

The trait is generic over the message type so the same `Transport` carries `OracleToLegion` from oracle's side and `LegionToOracle` from legion's side. Concrete impls don't need to know which direction they're carrying.

### Concrete impls

- **`SerialTransport`** (`feature = "serial"`): wraps `tokio_serial::SerialStream`. Reads bytes from the radio's serial port, runs them through a streaming COBS decoder, dispatches complete frames to postcard. Handles partial frames across multiple reads. Reopens the port on EOF.
- **`TcpTransport`** (`feature = "tcp"`): wraps `tokio::net::TcpStream`. Same COBS-postcard codec. Used for SITL, dev, oracle's per-test mock-legion, and any future IP-radio hardware.

Both are `std + tokio`. The future `legion-mcu` crate will provide an `EmbassyUartTransport` that lives there (not here), implementing the same `Transport` trait against `embassy-stm32-usart` or similar.

## Versioning

The `Envelope.v` field carries `PROTOCOL_VERSION: u8`, currently `1`.

- **Version mismatch in `Hello`** → the receiver sends an `Error { code: "version_mismatch", message: ... }` frame and closes the connection. Both binaries log it loudly.
- **Backward-compatible additions** (new optional fields, new enum variants at the *end* of an enum, new message variants) bump no version. Old receivers will fail to deserialize the new variants and report `Error { code: "unknown_variant" }`, which is acceptable for v1.
- **Breaking changes** (renamed fields, reordered enum variants, removed messages, changed numeric types) bump `PROTOCOL_VERSION` and require coordinated upgrades on both sides.

For v1 we keep this informal — there's one operator and two engineers, mismatches get caught at the workspace `cargo check` boundary. v2 (with deployed drones in the field that may run older legion builds) will need a formal compatibility matrix.

## Feature flags

| Flag | Default | Purpose | Brings in |
|---|---|---|---|
| (none) | yes | The type definitions and the codec only. `no_std + alloc`. | `serde`, `serde_derive`, `postcard`, `cobs` |
| `tcp` | no | `TcpTransport` impl | `tokio` (with `net` and `io-util` features) |
| `serial` | no | `SerialTransport` impl | `tokio`, `tokio-serial` |
| `proptest` | no | Property-based test helpers (round-trip generators for every type) | `proptest` |

`legion-core` depends on the crate with default features (no transports). `oracle` and `legion` depend with `features = ["serial", "tcp"]`.

## Testing

The crate ships its own tests, inherited by both consumers:

1. **Unit tests** for individual type round-trips (`Sortie`, `SortieStep`, every `OracleToLegion` and `LegionToOracle` variant). Runs in milliseconds.
2. **Property-based tests** via `proptest` (under the `proptest` feature) that generate arbitrary message values and assert `decode(encode(m)) == m` for the full pipeline (postcard + COBS).
3. **Wire-byte fixture tests** that pin the byte representation of a small set of canonical messages. These catch accidental wire-format changes — any postcard or COBS upgrade that changes the encoding will trip them and force a deliberate decision.
4. **Streaming-decoder tests** that feed the COBS decoder with arbitrarily-chunked input (one byte at a time, two bytes, half a frame, etc.) and verify it produces the same frames as the all-at-once case. This is the test that catches partial-read bugs in `SerialTransport`.

## Adding a new message

1. Add the variant to the appropriate enum in `messages.rs`.
2. If the variant introduces new types, define them in the appropriate module (`sortie.rs`, `telemetry.rs`, etc.).
3. Add a round-trip unit test.
4. If the type is non-trivial, add a `proptest` strategy.
5. Update the catalogue table in this README.
6. Both consumers (`oracle` and `legion`) get the new variant on the next `cargo build`. Add handling in the relevant supervisor / executor.

There is no separate "deploy the schema" step. The crate is the schema.

## What this crate deliberately doesn't have

- **No serde JSON.** The wire is binary; the typed `Debug` impls plus the `--protocol-debug` mode in each binary handle the human-readable case.
- **No async runtime.** The trait uses stable `async fn in trait`; the runtime is supplied by the consumer.
- **No business logic.** The executor lives in `legion-core`, the slicer lives in `oracle`. This crate only knows about *bytes on the wire*.
- **No persistence.** The wire types are not the storage types — oracle has its own SQLite schema (see [oracle/README.md → Persistence](../oracle/README.md#persistence)) and legion has its own JSON files. The crate's types may be serialized to either, but the storage layer is each consumer's responsibility.
- **No 915 MHz hardware assumptions.** The crate is transport-agnostic; the radio choice lives in oracle's and legion's READMEs (EU-legal SiK radios, see [legion's topology section](../legion/README.md#topology)).
