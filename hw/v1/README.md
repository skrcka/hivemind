# Hivemind hardware — v1 (proof of concept)

> Parent: [hw/README.md](../README.md)

**Goal:** fly one drone to a wall, spray paint on it, come back. Everything runs on one laptop.

v1 deliberately strips out everything that isn't load-bearing for that goal. No RTK, no swarm, no oracle service, no pantheon UI, no automated refill. The point of v1 is to validate the *physical pipeline* — frame, motors, payload, pump, nozzle, ToF sensor, MAVLink loop — on hardware that costs less than €800.

## Ground station

| Item | Specific product | Role | Price |
|---|---|---|---|
| Laptop | Existing Ubuntu laptop | Runs QGroundControl, Python scripts, everything | €0 |
| Telemetry radio (ground) | SiK 868 MHz (included in X500 kit) | USB dongle — talks MAVLink to drone | included |
| Game controller | Xbox 360 USB controller | Manual flight override via QGroundControl | €15 |
| Battery charger | ISDT Q6 Nano or equivalent | Charges 4S LiPo batteries between flights | €30 |

**v1 ground total: ~€45**

## Worker drone (×1)

### Flight platform

| Item | Specific product | Role | Price |
|---|---|---|---|
| Frame + motors + ESCs + PDB + props | Holybro X500 V2 frame kit | Physical structure, pre-assembled arms with motors | incl. |
| Flight controller | Holybro Pixhawk 6C | Runs PX4, stabilizes flight, follows waypoints | incl. |
| GPS | Holybro M10 GPS | Standard positioning (±2 m) — fine for v1 | incl. |
| Telemetry radio (drone) | SiK 868 MHz | Talks to laptop for QGroundControl telemetry | incl. |
| **Kit price** | **Holybro PX4 Dev Kit X500 V2** | **All of the above in one box, ~30 min assembly** | **€450** |
| Batteries ×4 | 4S 2200 mAh LiPo (Tattu / CNHL) | ~10 min flight time each, swap between flights | €100 |
| Propellers (spare) | 10×4.5", 5 extra sets | Consumable — expect crashes during testing | €15 |

### Companion computer

| Item | Specific product | Role | Price |
|---|---|---|---|
| Computer | Raspberry Pi 5 (4 GB) | Runs the legion agent — Python code that controls the mission | €80 |
| SD card | 32 GB microSD | OS + code | €10 |
| Serial cable | JST-GH to dupont / UART cable | Connects Pi to Pixhawk for MAVLink | €5 |
| WiFi | Built-in Pi WiFi | Link to laptop (phone hotspot or portable router) | €0 |
| Mounting | M3 standoffs + nylon screws | Mounts Pi to X500's companion-computer plate | €5 |

### Spray payload

v1 uses an off-the-shelf aerosol spray paint can actuated by an SG90 servo. The mechanism is dirt cheap (~€10), has zero priming, zero clog risk, and a 15-second can swap. Full build doc, wiring, and software lives in **[hw/nozzle/README.md](../nozzle/README.md)** — read that before sourcing parts.

| Item | Specific product | Role | Price |
|---|---|---|---|
| Servo | SG90 180° positional, 9 g | Pushes the spray can nozzle down on command. Driven by Pixhawk AUX5 (preferred) or Pi GPIO 18. | €3 |
| Spray can | Standard 400 ml aerosol paint, hardware store | The whole payload. Spring-loaded nozzle does the spraying when the servo arm presses it. | €4 |
| Hose clamp | 60–80 mm adjustable metal band | Mounts the can vertically to the X500 frame, nozzle pointing down, near the drone CG. | €1 |
| L-bracket | Aluminium or 3D-printed | Mounts the SG90 to the frame so the arm can reach the nozzle button. | €2 |
| Fasteners | M3 screws + nuts (4×) | Bracket and servo mounting | €1 |
| Distance sensor | TFMini-S LiDAR or VL53L1X ToF | Measures distance to the wall surface. v1 = data logging. Required even in v1 because it's the input the v2 Layer 3 local safety loop will use against real surfaces. | €20 |
| Wiring | Misc connectors, solder, heatshrink | Servo signal lead, ToF sensor cable | €5 |
| Spare cans | Box of 12 | Consumables for a day of testing | included in operating cost |

### Per-drone total

| Category | Cost |
|---|---|
| Holybro X500 V2 kit (frame, FC, motors, ESC, GPS, radio, props) | €450 |
| Batteries ×4 + spare props | €115 |
| Raspberry Pi 5 + SD + cables + mounting | €100 |
| Spray mechanism (servo, can, clamp, bracket, fasteners) | €11 |
| Distance sensor + wiring | €25 |
| **Drone total** | **€701** |

## v1 complete system

| | Cost |
|---|---|
| Ground station (charger + controller) | €45 |
| Worker drone ×1 | €701 |
| **v1 total** | **~€746** |

## What v1 doesn't have (and why that's OK)

| Skipped | Why OK for v1 |
|---|---|
| RTK GPS | ±2 m is fine for "fly to wall, spray." Not painting precise lines yet. |
| Multiple drones | Get one working. Swarm is a software problem for later. |
| Oracle service | The operator *is* oracle. SSH into the Pi, run a Python script. |
| Pantheon UI | QGroundControl for telemetry. Hardcode GPS coordinates in the script. |
| 3D scanning | Use a flat wall for testing. |
| Plan/apply workflow | `python3 spray_mission.py` is the plan and the apply. |
| Automated refill | Walk to drone, swap can + battery in ~15 s. |
| Paint level sensor | Count seconds of spray time, swap the can if in doubt. |
| Rugged tablet | Laptop on a folding table at the field. |
| Environmental containment | Test on your own wall in a field. |

## What v1 still has to get right

The list above is what v1 *omits*. The list below is what v1 absolutely cannot omit, because it's the point of building v1 in the first place:

- **Servo → can → spray pattern.** Verify the SG90 + aerosol can combo produces a usable spray pattern at the standoff range you'll fly. Log the standoff↔pattern-width table — that's the input the v2 planner needs to lay out lanes. Detailed test procedure in [hw/nozzle/README.md](../nozzle/README.md#testing-procedure).
- **ToF sensor data quality.** Even though v1 only logs it, the sensor has to produce stable, low-noise distance readings against a real surface (with paint, in sunlight). This is what enables Layer 3 local safety later.
- **MAVLink offboard loop.** Pi → Pixhawk → motor command must be tight enough to do precise position holds against the wall. This is the same loop the production system relies on; if it doesn't work in v1 it won't work in v2.
- **Battery margin with payload.** 10-minute flight time *with* the spray mechanism installed and a full 350 g spray can. Not bench numbers.
- **CG with the can mounted.** The can is the heaviest single payload item by an order of magnitude. Get it on the centre of gravity or the drone fights itself the whole flight.
- **Crash recovery procedure.** Spare props, spare ESCs, repair kit. v1 will crash. Plan for it.
