# praetor

**Direct manual-control Tauri client for Hivemind drones.** Takes Xbox controller input and drives one drone's Pixhawk directly over MAVLink — independent of oracle, legion, and pantheon.

> See the top-level [README](../README.md) for project context. This crate is the **independent backup channel** described in [legion/README.md](../legion/README.md) and [hw/README.md](../hw/README.md): a parallel MAVLink link to the Pixhawk's TELEM1 port that keeps working when oracle or legion are dead, and gives the operator a manual override path over the normal plan/apply lifecycle.

## Role

Praetor is the operator-with-a-joystick. It is the only code in Hivemind that:

- Talks MAVLink on the ground side (oracle uses the custom `hivemind-protocol`; legion uses MAVLink only on the drone-side TELEM2 UART, never on a radio).
- Sends commands that did not come from an Approved plan — bypasses oracle's [`CommandAuthority`](../oracle/README.md#commandauthority) choke point entirely.
- Reads Xbox controller input and translates it to flight commands in real time.
- Runs on the truck laptop / NUC alongside oracle but shares no state with it.

The name: a **praetor** in the Roman cursus honorum held *imperium* — the authority to command troops in the field. Matches the classical theme (`pantheon`, `oracle`, `legion`, `vanguard`) and captures the role — the operator who can override the regular chain of command.

## Why this exists

Three scenarios drive the design:

1. **Oracle or legion is dead and the drone is in the air.** The operator needs a way to get the drone back without the normal stack. Without praetor, the only option is flipping the transmitter switch (if a human pilot is even on-site), or waiting for PX4's failsafe RTL.
2. **Bench testing of payload hardware.** A dev is validating the spray pump, the ToF sensor, or a new airframe configuration. Spinning up the entire oracle/legion/pantheon stack just to hover a drone and trigger the pump is a waste of time.
3. **Early bring-up of a new drone.** Before a drone has ever successfully executed a sortie, it needs to fly under manual control to tune PIDs, verify RC/RTK, and check that the flight controller is configured correctly.

Every one of these wants the same thing: **a direct operator-to-drone loop, with standard protocols, independent of the rest of the stack**.

## Architecture

```
┌──────────────────── Truck NUC / laptop ────────────────────┐
│                                                             │
│  ┌──────────── Praetor (Tauri) ─────────────┐  ┌─────────┐  │
│  │                                           │  │ oracle  │  │
│  │  React HUD  ◀──── Tauri events ──── Rust │  │         │  │
│  │             ──── #[tauri::command] ────▶  │  └─────────┘  │
│  │                                           │  ┌─────────┐  │
│  │  - gilrs gamepad poller (100 Hz)          │  │ pantheon│  │
│  │  - MAVLink connection (serial or TCP)     │  │         │  │
│  │  - safety watchdogs                       │  └─────────┘  │
│  │  - arming / mode state                    │                │
│  └──────────┬────────────────────────────────┘                │
│             │ /dev/ttyUSB1  (second SiK radio)                │
│             │ or tcp://127.0.0.1:5760 (PX4 SITL)              │
└─────────────┼───────────────────────────────────────────────────┘
              │
              │ MAVLink v2, common.xml dialect
              │
┌─────────────┼─────────────── Drone ───────────────────────────┐
│             ▼                                                  │
│    ┌──────────────┐                  ┌────────────────────┐    │
│    │ SiK radio    │──── UART ── TELEM1│                    │    │
│    │  (separate   │                  │   Pixhawk 6C       │    │
│    │  from legion)│                  │   (PX4)            │    │
│    └──────────────┘                  │                    │    │
│                                      │  TELEM2 ─── Pi     │    │
│                                      │  (legion, parallel)│    │
│                                      │                    │    │
│                                      │  AUX5 ─── servo    │    │
│                                      │          (pump)    │    │
│                                      └────────────────────┘    │
└──────────────────────────────────────────────────────────────┘
```

Key properties:

1. **Second SiK radio, distinct from legion's.** Legion owns TELEM2; praetor owns TELEM1. No shared fate.
2. **Standard MAVLink dialect (`common.xml`).** No custom frames. PX4 speaks it natively.
3. **One drone at a time.** Praetor is not a fleet manager. Run one praetor per drone if you need to manually fly several (which you shouldn't — manual control multiple drones simultaneously is not a safe workflow).
4. **Mode handoff via `MAV_CMD_DO_SET_MODE`.** Praetor does not barge into an active sortie. The operator switches PX4 out of Offboard mode (which legion drives) into a manual flight mode; legion's executor sees the mode change and pauses its step handlers. To hand back, praetor switches PX4 back into Offboard, and legion resumes.
5. **No shared state with oracle.** Praetor does not subscribe to oracle's WebSocket, does not POST to oracle's REST API, does not read intent files. If oracle is healthy, the operator can view oracle's fleet page in parallel — but praetor itself is self-contained.

## Pump control path

The spray servo is wired to **Pixhawk AUX5** (the only supported wiring per [project_hardware.md](../hw/nozzle/README.md)). Praetor and legion both hit it through the same standard MAVLink command — `MAV_CMD_DO_SET_SERVO` with `servo_index = 5` and PWM 2000 (ON) / 1000 (OFF). This gives us a single control path, deterministic PWM, reboot safety, and the PX4 servo failsafes. There is no Pi-GPIO alternative.

Praetor can drive the pump this way independently of legion — it works even if the Pi is dead — because the command goes straight to the Pixhawk over TELEM1.

See the [Xbox controller mapping](#xbox-controller-mapping) below for which button fires the pump, and the [Safety interlocks](#safety-interlocks) section for when the pump command is *refused*.

> **Corresponding PX4 setup**: `PWM_AUX_FUNC5` must be set to the generic "Servo" output function so AUX5 responds to `MAV_CMD_DO_SET_SERVO`. Default PX4 config on the Pixhawk 6C already exposes AUX outputs as servo channels.

## Tech stack

| Layer | Crate / library | Why |
|---|---|---|
| App shell | [`tauri 2`](https://tauri.app) | Single binary, cross-platform, Rust backend |
| Async runtime | [`tokio`](https://tokio.rs) (multi-thread) | Matches oracle / legion |
| Gamepad input | [`gilrs`](https://docs.rs/gilrs) | Cross-platform (Xbox 360/One/Series on macOS/Linux/Windows), event-driven |
| MAVLink | [`rust-mavlink`](https://github.com/mavlink/rust-mavlink) | Same crate legion uses; typed messages from `common.xml` |
| Serial | [`tokio-serial`](https://docs.rs/tokio-serial) | Same crate legion uses |
| Config | [`figment`](https://docs.rs/figment) (TOML + env) | Matches oracle |
| Logging | [`tracing`](https://docs.rs/tracing) | Matches oracle / legion |
| Errors | `thiserror` + `anyhow` | Matches project conventions |
| Frontend | React + TypeScript + Vite | Matches long-term pantheon direction |
| HUD graphics | SVG / CSS (no UI library) | Minimal deps; HUD is ~6 small components |

## Module layout

```
praetor/
├── Cargo.toml                 ← excluded from the workspace (like oracle)
├── tauri.conf.json            ← Tauri 2 config
├── build.rs                   ← tauri_build::build()
├── README.md                  ← this file
│
├── src/                       ← Rust backend
│   ├── main.rs                ← Tauri entry: builder, plugin registration, task spawning
│   ├── lib.rs                 ← re-exports for integration tests
│   │
│   ├── config.rs              ← figment-loaded Config (serial port, bindings, timeouts)
│   ├── error.rs               ← thiserror enum
│   ├── state.rs               ← AppState: telemetry snapshot, link status, arming state
│   │
│   ├── gamepad/
│   │   ├── mod.rs             ← gilrs poller task (100 Hz) publishing ControlIntent
│   │   ├── binding.rs         ← axis/button → action mapping table
│   │   └── intent.rs          ← ControlIntent struct (normalized axes + button latches)
│   │
│   ├── mavlink_link/
│   │   ├── mod.rs             ← MavlinkLink handle wrapping the connection + tasks
│   │   ├── connect.rs         ← open the connection, wait for first HEARTBEAT
│   │   ├── send.rs            ← MANUAL_CONTROL / COMMAND_LONG / DO_SET_ACTUATOR builders
│   │   ├── recv.rs            ← inbound parser: HEARTBEAT, ATTITUDE, GPS_RAW_INT,
│   │   │                        BATTERY_STATUS, GLOBAL_POSITION_INT, DISTANCE_SENSOR,
│   │   │                        RADIO_STATUS, STATUSTEXT, SYS_STATUS
│   │   └── snapshot.rs        ← TelemetrySnapshot: the struct the HUD renders from
│   │
│   ├── safety/
│   │   ├── mod.rs             ← composed safety loop
│   │   ├── arming.rs          ← hold-to-arm state machine
│   │   ├── watchdog.rs        ← controller-silent + link-silent timers
│   │   └── interlock.rs       ← pump-requires-armed-and-airborne, etc.
│   │
│   └── tauri_commands.rs      ← #[tauri::command] handlers + event emitter task
│
└── ui/                        ← React + TypeScript + Vite
    ├── package.json
    ├── index.html
    ├── vite.config.ts
    ├── tsconfig.json
    └── src/
        ├── main.tsx
        ├── App.tsx
        ├── App.css
        ├── lib/tauri.ts                 ← typed event subscription / command invocation
        ├── hooks/
        │   ├── useTelemetry.ts
        │   ├── useLinkStatus.ts
        │   └── useControllerStatus.ts
        └── components/
            ├── Hud/
            │   ├── AttitudeIndicator.tsx
            │   ├── BatteryGauge.tsx
            │   ├── GpsBadge.tsx
            │   └── ModeBadge.tsx
            ├── ConnectionPanel.tsx
            ├── ArmButton.tsx
            ├── EmergencyStop.tsx
            └── PumpIndicator.tsx
```

## Xbox controller mapping

Defaults, all configurable in `config.toml` under `[gamepad.bindings]`.

| Input | Action | MAVLink |
|---|---|---|
| Left stick X | Roll | `MANUAL_CONTROL.y` |
| Left stick Y | Pitch | `MANUAL_CONTROL.x` (inverted) |
| Right stick Y | Throttle | `MANUAL_CONTROL.z` |
| Right stick X | Yaw | `MANUAL_CONTROL.r` |
| D-pad ←/→ | Fine lateral nudge | overrides roll axis for 0.5 s |
| D-pad ↑/↓ | Fine pitch nudge | overrides pitch axis for 0.5 s |
| **A** (hold) | Spray pump ON | `MAV_CMD_DO_SET_SERVO` servo 5 = 2000 µs |
| A (release) | Spray pump OFF | `MAV_CMD_DO_SET_SERVO` servo 5 = 1000 µs |
| **B** | Return to launch | `MAV_CMD_NAV_RETURN_TO_LAUNCH` |
| **Y** | Takeoff | `MAV_CMD_NAV_TAKEOFF`, param7 = 2 m |
| **X** | Land | `MAV_CMD_NAV_LAND` |
| LB + RB (hold 3 s) | **Arm** | `MAV_CMD_COMPONENT_ARM_DISARM` param1=1 |
| LB + RB (double tap) | Disarm | `MAV_CMD_COMPONENT_ARM_DISARM` param1=0 |
| Start | Cycle flight mode | `MAV_CMD_DO_SET_MODE` |
| **Back** (hold 1 s) | **EMERGENCY MOTOR CUT** | `MAV_CMD_COMPONENT_ARM_DISARM` param1=0 param2=21196 |
| LT (analog) | Alt throttle (optional) | overrides right stick Y if pressed |

`MANUAL_CONTROL` axes are normalized to `-1000..1000` (and `0..1000` for throttle `z`). `MANUAL_CONTROL` is sent at 20 Hz whenever the drone is armed.

## Safety interlocks

Every interlock is enforced on the Rust side. The UI cannot bypass them by constructing events directly.

1. **Hold-to-arm** — arming requires holding LB+RB for 3 seconds. No single-button arming. UI shows a circular progress ring during the hold.
2. **Controller-silent watchdog** — if gilrs reports no input events for >1 s, praetor switches to `MANUAL_CONTROL { x=0, y=0, z=500, r=0 }` (centred sticks, neutral throttle) and sends it at the same 20 Hz. If the drone is in a self-stabilising mode (Position / Altitude) this reduces to "hold position." After 3 s of silence, praetor additionally sends `MAV_CMD_NAV_LOITER_UNLIM`.
3. **Link-silent watchdog** — if no `HEARTBEAT` from the Pixhawk for >3 s, the UI flashes red and all commands are rejected. PX4's own RC-loss failsafe handles the drone side (configured at provisioning).
4. **Pump interlock** — `DO_SET_ACTUATOR` for pump-on is rejected unless:
   - the drone is armed
   - the reported altitude is > 0.5 m relative to takeoff
   - the link is healthy (last HEARTBEAT ≤ 1 s ago)
   If any of these fails, the pump command silently becomes a no-op and the UI shows a warning.
5. **Emergency stop** — the big red button + Back controller binding sends arm/disarm with `param2 = 21196.0` (PX4's documented "force kill in flight" magic). Requires a 1-second hold in the UI, and gets its own audit log entry.
6. **Single-drone lock** — config specifies one `drone_system_id`; frames from any other system ID are logged and ignored. Refuses to connect if multiple system IDs show up.
7. **Mode-handoff gate** — `MANUAL_CONTROL` frames are NOT sent until PX4 is confirmed in a manual-capable mode. The operator has to explicitly switch out of Offboard (Start button) before sticks become live.
8. **Audit log** — every outbound command is written to `praetor-session-<timestamp>.jsonl` with the telemetry snapshot at dispatch time.

## Configuration

`praetor.toml` (lookup order: `./praetor.toml` → `$HOME/.config/praetor/praetor.toml` → env `PRAETOR_*` overrides).

```toml
[link]
# Either a serial port (second SiK radio):
address = "serial:/dev/ttyUSB1:57600"
# …or a PX4 SITL TCP endpoint:
# address = "tcp:127.0.0.1:5760"

drone_system_id = 1
target_component_id = 1

[gamepad]
poll_hz = 100
silent_threshold_s = 1.0
hard_silent_threshold_s = 3.0

[link.watchdog]
link_silent_threshold_s = 3.0

[safety]
arm_hold_duration_s = 3.0
emergency_stop_hold_s = 1.0
pump_minimum_altitude_m = 0.5

[pump]
# Pixhawk servo output channel the nozzle servo is wired to. Sent as
# `MAV_CMD_DO_SET_SERVO.param1`. For v1 hardware this is 5 (AUX5). This
# is the only supported wiring — do not plumb a Pi-GPIO alternative.
servo_index = 5
pwm_on_us  = 2000
pwm_off_us = 1000

[takeoff]
default_altitude_m = 2.0

[gamepad.bindings]
# axis mappings (all normalized -1.0 .. 1.0)
roll         = { axis = "LeftStickX" }
pitch        = { axis = "LeftStickY", invert = true }
throttle     = { axis = "RightStickY" }
yaw          = { axis = "RightStickX" }

# button mappings
pump          = "South"   # A on Xbox
rtl           = "East"    # B
takeoff       = "North"   # Y
land          = "West"    # X
arm_combo     = ["LeftTrigger2", "RightTrigger2"]  # actually LB + RB
mode_cycle    = "Start"
emergency     = "Select"  # Back
```

## Running

### Against PX4 SITL (no hardware needed)

```bash
# Terminal 1 — PX4 SITL
cd PX4-Autopilot
make px4_sitl gazebo_classic   # opens MAVLink on tcp://127.0.0.1:5760

# Terminal 2 — praetor
cd hivemind/praetor
# Edit praetor.toml: address = "tcp:127.0.0.1:5760"
cargo tauri dev
```

### Against the real v1 drone

```bash
# Plug the second SiK radio into the truck laptop (/dev/ttyUSB1 on Linux,
# /dev/tty.usbserial-XXXX on macOS). The drone's TELEM1 has the matching radio.

cd hivemind/praetor
cargo tauri dev
# Connect via the UI's port picker, wait for HEARTBEAT, proceed.
```

## What praetor deliberately doesn't do

- **No fleet management.** One drone at a time.
- **No oracle / pantheon integration.** No WebSocket subscribers, no REST clients.
- **No plan / apply lifecycle.** This is the skip-the-lifecycle-and-fly-the-thing tool.
- **No mission upload.** No waypoint lists, no surveys.
- **No 3D scene view.** This is a HUD, not pantheon.
- **No paint-level sensing.** The HX711 load cell lives on the Pi (legion). v1 of praetor cannot see paint level via the Pixhawk MAVLink stream alone. If the operator needs paint level during manual ops, they look at oracle's fleet view in parallel.
- **No ToF-based wall avoidance.** Layer 3 local safety lives in legion's on-drone safety loop and cannot be accessed from the MAVLink ground side. Praetor trusts the operator to not fly into walls.
- **No negotiation with legion.** Mode handoff is "operator is responsible" — legion's executor pauses cleanly when PX4 leaves Offboard mode, but praetor doesn't coordinate with legion directly.

## Testing

| Layer | How tested | When |
|---|---|---|
| Pure modules (binding parsing, intent normalization, interlock predicates) | `cargo test` | Every commit |
| MAVLink send/recv round-trip | PX4 SITL (`tcp:127.0.0.1:5760`) | Every push |
| Gamepad event handling | `cargo test` with mocked gilrs events | Every commit |
| Flight commands end-to-end | PX4 SITL with a scripted verifier | Pre-merge (nightly) |
| Pump via AUX5 | Bench test with the hw/nozzle test rig | Before each v1 drone release |
| Real hardware | v1 X500 dev kit + second SiK radio | Once per phase |

## Status

**Phase 0** — scaffold, config, React HUD shell, gilrs reading to stdout. ← current

Phase 1 — read-only HUD from MAVLink SITL. Phase 2 — flight commands. Phase 3 — pump via DO_SET_ACTUATOR. Phase 4 — real drone validation.
