# Hivemind hardware — v2 (production)

> Parent: [hw/README.md](../README.md) · Previous phase: [v1](../v1/README.md)

**Goal:** 10-drone swarm painting a real bridge commercially.

v2 is the full system: rugged ground station on the truck, 10 worker drones with RTK, a scout drone for vanguard, a paint refill station, and the physical site layout. Total ~€13K of hardware.

The architectural difference from v1 is not "more drones." It is **everything that has to be true to do a job a customer pays for**: cm-accuracy GPS, weather-proof equipment, real fleet telemetry, automated refill, safe site layout, and a recovery story for every component that can fail.

## Ground station (on the truck)

| Item | Specific product | Role | Price |
|---|---|---|---|
| Compute | Intel NUC 13 Pro (i7, 32 GB, 1 TB) | Runs oracle + Skybrush Server + telemetry aggregation | €700 |
| Operator tablet | Getac F110 or Dell Latitude 7230 Rugged | Runs pantheon UI. Sunlight-readable, IP65, touchscreen. | €1,200 |
| Controller | Xbox controller | Manual override for any individual drone | €15 |
| RTK base station | ArduSimple RTK Smart Antenna (u-blox F9P) | Broadcasts RTCM3 corrections to every drone. On a tripod on the truck roof. Gives the fleet ±2 cm GPS accuracy. | €400 |
| RTK tripod | Survey tripod with 5/8" thread | Stable mount for the RTK antenna with clear sky view | €50 |
| Telemetry | WiFi mesh router (dedicated 5 GHz) | Communication backbone for oracle ↔ all legion agents | €100 |
| Battery charger | SkyRC Q200 (4-port parallel) | Charges 4 batteries simultaneously. Runs off truck 12 V via inverter. | €150 |
| Batteries | 4S 2200 mAh × 25 | 2–3 per drone in rotation, plus charging buffer | €625 |
| Paint refill station | Custom: 20 L reservoir + 12 V peristaltic pump + scale + quick-connect fittings | Refills drone cartridges to exactly 500 g. Controlled by ESP32. | €400 |
| Power inverter | 1500 W pure sine wave | Truck 12 V → 230 V AC for charger and NUC | €100 |
| Landing pads ×10 | Numbered foam pads with ArUco markers | Defined landing spots near the truck. Markers prepare for future precision landing. | €100 |
| Portable router | GL.iNet travel router | Dedicated WiFi network for the swarm. No dependency on cellular. | €60 |
| Cables, spares, tools | Misc | XT60 connectors, spare props, hex keys, multimeter, zip ties | €200 |

**Ground station total: ~€4,100**

The single most important line item is the **RTK base station**. Without it the swarm has ±2 m GPS accuracy and the entire industrial-painting use case falls over. With it the fleet shares a ±2 cm reference. See [the main README's spatial alignment section](../../README.md#spatial-alignment-zeroing) for the full rationale.

## Worker drone (×10)

### Flight platform

| Item | Specific product | Role | Price |
|---|---|---|---|
| Frame | Custom 500 mm carbon fiber (or Holybro S500) | Optimized for payload: bottom-mount spray system, water-resistant motor area, quick-release cartridge bay | €100 |
| Flight controller | Holybro Pixhawk 6C Mini | PX4 firmware. Triple-redundant IMU. Same as v1 but Mini form factor saves weight. | €150 |
| GPS with RTK | Holybro H-RTK F9P | Receives RTK corrections from the base station. ±2 cm accuracy. **The critical upgrade from v1.** | €200 |
| Motors ×4 | T-Motor 2216 880 KV | Sized for 10" props on 4S, ~800 g thrust each = 3.2 kg total | €60 |
| ESCs ×4 | BLHeli_32 30 A | DShot protocol to Pixhawk, smooth motor control | €40 |
| Propellers | 10×4.5" carbon fiber, 10 sets per drone | Consumable. Carbon fiber for durability in industrial use. | €30 |
| Battery | 4S 2200 mAh LiPo | Quick-swap: velcro strap + XT60. ~10 min flight with payload. Pooled with ground charger. | (pooled in ground station) |
| PDB + wiring | Matek PDB + XT60 / XT30 | Power distribution: battery → ESCs, 5 V BEC → Pi + Pixhawk, 12 V → pump via relay | €15 |

### Companion computer

| Item | Specific product | Role | Price |
|---|---|---|---|
| Computer | Raspberry Pi 5 (4 GB) | Runs the legion agent. Receives sorties from oracle, feeds waypoints to PX4, controls pump, reads sensors, reports telemetry. | €80 |
| WiFi adapter | USB WiFi with external antenna | Stronger link to the ground station than the Pi's internal WiFi. Antenna mounted on top of the frame for clear LoS. | €20 |
| SD card + case | 32 GB + Pi case | Conformal coated or potted for vibration / moisture in production | €15 |
| Serial cable | UART (Pi ↔ Pixhawk) | MAVLink at 921600 baud | €5 |

### Spray payload (production)

v2 replaces v1's [servo + aerosol can mechanism](../nozzle/README.md) with a real pump + cartridge system. The reason is paint chemistry, not engineering preference: industrial bridge coatings (zinc-rich primers, epoxy intermediates, polyurethane topcoats) do not come in aerosol cans. They need a positive-displacement pump and flow-rate control. v1's mechanism remains the canonical bench-test rig and is documented in [hw/nozzle](../nozzle/README.md).

| Item | Specific product | Role | Price |
|---|---|---|---|
| Paint cartridge | Custom 500 ml bayonet-lock bladder | Quick-release twist-lock on the drone underside. Operator swaps in 10 s. Transparent for visual check. Collapsible bladder (not a rigid bottle) to avoid air bubbles. | €10 |
| Pump | 12 V peristaltic pump (100 ml/min, ~50 g) | Paint only touches silicone tube → no clog. PWM flow-rate control from Pi GPIO (this is the v2 capability v1's on/off servo could not provide). | €15 |
| Relay / MOSFET | Logic-level MOSFET module | Switches the pump from Pi GPIO. MOSFET preferred over relay for PWM speed control. | €5 |
| Nozzle | Flat-fan agricultural tip (TeeJet 8002) | 80° fan, ~25 cm pattern at 50 cm standoff, consistent edge-to-edge distribution | €5 |
| Distance sensor | TFMini-S LiDAR | Maintains 50 cm standoff from the surface. Feeds into PX4 / legion for altitude hold relative to the wall. 10 Hz update. | €25 |
| Paint level sensor | HX711 load cell under the cartridge mount | Oracle knows exact paint remaining → accurate sortie scheduling | €5 |
| Tubing | 6 mm ID silicone (spare sets) | Replaceable: when paint builds up, swap the tube segment (~€1) | €5 |
| Drip guard | 3D-printed shroud around the nozzle | Catches drips when the pump stops. Prevents paint on the drone. | €2 |

### Per-drone total (production)

| Category | Cost |
|---|---|
| Flight platform (frame, FC, RTK GPS, motors, ESCs, props, PDB) | €595 |
| Companion computer (Pi, WiFi, SD, cable) | €120 |
| Spray system (cartridge, pump, MOSFET, nozzle, sensors, tubing, guard) | €72 |
| **Per drone** | **€787** |
| **× 10 drones** | **€7,870** |

## Scout drone (vanguard, ×1)

Three options, picked per job:

| Option | What | Price | Notes |
|---|---|---|---|
| **A — Commercial** | DJI Air 3 | €900 | Quick photogrammetry. Photos → OpenDroneMap. Not open source but works immediately. |
| **B — DIY** | Holybro X500 + camera gimbal + Sony RX0 II | €800 | Open source. Same platform as worker drones. Survey patterns via QGroundControl. |
| **C — iPhone** | iPhone Pro (existing) + Scaniverse | €0 | Walk around smaller structures. Free. Good for prototyping the scan pipeline. |

For RTK-georeferenced scans (which is what the spatial-alignment pipeline assumes), Option B is the cleanest because it can carry the same H-RTK F9P GPS as the worker drones and emit GPS-EXIF-tagged photos directly.

## Production complete system

| | Cost |
|---|---|
| Ground station | €4,100 |
| Worker drones × 10 | €7,870 |
| Scout drone | €900 |
| **Production total** | **~€12,870** |

## Physical layout at the job site

```
                        BRIDGE
    ┌═══════════════════════════════════════════┐
    ║                                           ║
    ║   Drone 01 ──┐    Drone 02 ──┐            ║
    ║   (painting)  │    (painting)  │           ║
    ║   Lane 1      │    Lane 2      │           ║
    ║               │                │           ║
    ║               │                │   Drone 03
    ║               │                │   (returning
    ║               │                │    to refill)
    ║               │                │       │
    └═══════════════╪════════════════╪═══════╪═╝
                    │                │       │
                    │   Approach     │       │
                    │   corridors    │       │
                    │   (deconflicted)        │
                    │                │       │
    ────────────────┼────────────────┼───────┼── ROAD
                    │                │       │
                    ▼                ▼       ▼
    ┌─────────────────────────────────────────────────┐
    │                    TRUCK                        │
    │  ┌─────────┐ ┌──────────┐ ┌──────────────────┐  │
    │  │ RTK     │ │ Paint    │ │ Battery charger  │  │
    │  │ antenna │ │ refill   │ │ + spare batteries│  │
    │  │ (roof)  │ │ station  │ │                  │  │
    │  └─────────┘ └──────────┘ └──────────────────┘  │
    │  ┌──────────────────────────────────────────┐   │
    │  │ Operator station                         │   │
    │  │ NUC + tablet + controller                │   │
    │  │ (oracle + pantheon + skybrush)           │   │
    │  └──────────────────────────────────────────┘   │
    │                                                 │
    │  Landing pads:  [01] [02] [03] [04] [05]        │
    │                 [06] [07] [08] [09] [10]        │
    └─────────────────────────────────────────────────┘
```

The lane assignment shown here is what oracle's slicer guarantees at plan time (Layer 1 of the safety model). Approach corridors are deconflicted before any drone moves. See [oracle/README.md → Safety and deconfliction](../../oracle/README.md#safety-and-deconfliction).

## v1 → v2 upgrade path

| Component | v1 | v2 | Why upgrade |
|---|---|---|---|
| Ground compute | Existing laptop | NUC + rugged tablet | Weatherproof, dedicated, field-ready |
| GPS | Standard M10 (±2 m) | H-RTK F9P (±2 cm) | Precision painting requires cm accuracy |
| RTK base station | None | ArduSimple on tripod | Provides corrections for ±2 cm fleet GPS |
| Comms | SiK radio (1 drone) | WiFi mesh (10 drones) | Bandwidth for full fleet telemetry |
| Spray mechanism | SG90 servo pressing an aerosol can nozzle ([hw/nozzle](../nozzle/README.md)) | Peristaltic pump + bayonet cartridge with PWM flow control | Industrial coatings don't come in aerosol cans; need real flow rate control |
| Paint container | Standard 400 ml hardware-store spray can | Bayonet-lock quick-swap 500 ml bladder cartridge | 10 s swap vs 15 s clamp loosen; takes real industrial paint |
| Paint level | Count seconds of spray time | Load cell + HX711 | Oracle needs real data for sortie scheduling |
| Fleet size | 1 drone | 10 drones | Practical coverage speed |
| Oracle | Python script run manually | Persistent service with plan/apply API | Multi-drone scheduling, dynamic rebalancing |
| Pantheon | QGroundControl | Blender + Skybrush → custom Tauri app | 3D scan viewer, region painter, mission planning |
| Mission planning | Hardcoded GPS coordinates | Sliced from 3D scan | Real bridge geometry → automated toolpaths |
| Frame | Holybro X500 V2 kit | Custom carbon fiber | Optimized payload mount, water resistance, weight |
| Distance sensor | TFMini (data logging only) | TFMini feeding PX4 altitude hold | Auto standoff from surface |
| Landing | Anywhere flat | Numbered pads with ArUco markers | Future: precision auto-landing for refill dock |

## What v2 still has to get right (beyond v1's list)

- **RTK fix integrity end-to-end.** Base station → drone receivers → mission compiler all agree on the same datum. Tested before every job via the pre-flight visual alignment check (see [main README → at-job-site zeroing](../../README.md#at-job-site-zeroing)).
- **Refill cycle time.** Drone lands, operator swaps cartridge + battery, drone takes off. Target: <30 s total turnaround. Anything slower kills throughput.
- **WiFi link budget for 10 drones.** Continuous 5 Hz telemetry × 10 drones + RTK corrections + sortie uploads on a single dedicated 5 GHz network, with bridges and steel structures in the way. Validate at distance before promising customer coverage.
- **Spare-drone procedure.** A drone goes down mid-job. Oracle issues a `DroneDown` amendment, the operator swaps in a spare, the spare is provisioned with the right `RTL_RETURN_ALT` for its slot. This needs a written runbook.
- **Containment story.** Some kind of overspray management — even if it's "we only do water-based coatings" or "we deploy a netting drone first." Discussed in [main README → honest risks](../../README.md#honest-risks-and-caveats).
