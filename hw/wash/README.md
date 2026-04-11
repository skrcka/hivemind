# Hivemind hardware — wash payload (counter-thrust)

> Parent: [hw/README.md](../README.md) · Sibling payload: [hw/nozzle](../nozzle/README.md) (paint)

This is the canonical design doc for the **pressure-wash payload module**, the second of Hivemind's two payload variants. The first is the [paint nozzle](../nozzle/README.md). Both modules mount on the same drone platform; legion config tells each drone which one is attached.

The wash module is **designed but not built in v1**. v1 ships with the paint mechanism only, to validate the flight loop with a single drone. The wash module becomes practical once that flight loop works, and is documented now so the v2 platform decisions account for it (see [hw/v2 → Alternate payload modules](../v2/README.md)).

## The problem: pressure washing has reaction force

A 350 g aerosol spray can pushes ~50 g of reaction force when spraying — a rounding error against the drone's lift. PX4 absorbs it through normal tilt compensation and the operator never notices.

A pressure wash nozzle is different. Even a soft-wash nozzle at 1–2 L/min produces **~500–600 g of reaction force** continuously. Without compensation, the drone is shoved away from the wall the moment it starts spraying. PX4's attitude controller will fight back, but it does so by tilting the airframe — which means the wash spray pattern walks across the surface and the drone burns extra battery wrestling with itself. Long passes become impossible.

The fix is mechanical: **a counter-thrust fan on the opposite side of the drone, balancing the wash nozzle's reaction force in real time**. One extra motor that only runs while spraying.

## Concept

```
TOP VIEW:

              prop 1 (lift)          prop 2 (lift)
                 ○                      ○
                  \                    /
                   \                  /
                    ┌────────────────┐
     ◄◄◄◄ counter  │                │  wash nozzle ►►►►  wall
     prop/fan ─────┤     DRONE      ├─────────────────►   │
     (pushes air   │                │  (pushes water       │
      away from    │                │   toward wall)       │
      wall)        └────────────────┘                      │
                   /                  \
                  /                    \
                 ○                      ○
              prop 3 (lift)          prop 4 (lift)


SIDE VIEW:

         ▲ lift props ▲
         │            │
    ┌────────────────────┐
    │                    │
    │       drone        │
    │                    │
    └────────────────────┘
    ◄── counter     nozzle ──►
        prop          │
                      │
                    water
                    spray
                      ▼
```

The two horizontal forces cancel; the lift props see only normal gravity + small residual error. PX4 handles the residual through its existing attitude controller — the same way it copes with wind.

## Component selection — counter-thrust fan

The counter-prop has to produce **~500–600 g of thrust** to match a typical soft-wash nozzle at full flow. A ducted fan (EDF) wins over an open prop here because:

- More thrust per diameter — important since it mounts sideways on a frame that already has four lift props.
- Safer — no exposed spinning blade pointing outward at people or the structure.
- Cleaner airflow — the duct contains the wake so it doesn't recirculate into the lift props' downwash.

| Option | Thrust | Weight | Power | Price | Notes |
|---|---|---|---|---|---|
| **64 mm EDF** (recommended) | 600–900 g | ~100 g | ~200 W @ 600 g | €15–25 | 3S–4S, available on Amazon.de, fits the v2 4S battery |
| 50 mm EDF | 300–500 g | ~60 g | ~120 W | €10–15 | Marginal — may not match full wash pressure |
| 5" prop + 2306 motor (open) | ~800 g | ~50 g | ~150 W | €12 | More efficient but exposed blade — rejected on safety |

**Recommendation: 64 mm EDF.** Enough thrust headroom for a softer wash too, compact, safe, off-the-shelf.

## Smart control — proportional counter-thrust

The counter-prop is not on/off. It tracks the wash nozzle's commanded intensity in real time, so partial flows (e.g. starting up, tapering off the end of a pass, depressurizing as the tank empties) stay balanced:

```
Wash nozzle off    → counter-prop off     → net force: 0
Wash nozzle 50 %   → counter-prop 50 %    → net force: ~0
Wash nozzle 100 %  → counter-prop 100 %   → net force: ~0
```

The relationship is approximately linear within the operating range, with one calibration constant per drone (different fans, different nozzles, different water pressures all shift it slightly).

### Implementation in legion

Both the water valve and the counter-prop ESC plug into Pixhawk AUX outputs (just like the paint servo in [hw/nozzle](../nozzle/README.md)). Legion drives both together via MAVSDK actuator commands:

```python
# legion/wash.py — wash payload controller

class WashController:
    """Drives the wash payload: solenoid valve + counter-thrust EDF.

    The two actuators are kept in lockstep so the drone never sees an
    unbalanced reaction force from the water nozzle.
    """

    def __init__(
        self,
        valve_actuator: int,
        counter_prop_actuator: int,
        thrust_calibration: float,
    ) -> None:
        self.valve_actuator = valve_actuator
        self.counter_prop_actuator = counter_prop_actuator
        self.thrust_calibration = thrust_calibration

    async def wash_on(self, drone, intensity: float) -> None:
        """Open the water valve and spool the counter-prop to match.

        intensity: 0.0 (off) … 1.0 (full pressure)
        """
        await drone.action.set_actuator(self.valve_actuator, intensity)
        counter = intensity * self.thrust_calibration
        await drone.action.set_actuator(self.counter_prop_actuator, counter)

    async def wash_off(self, drone) -> None:
        """Close the valve and stop the counter-prop."""
        await drone.action.set_actuator(self.valve_actuator, 0.0)
        await drone.action.set_actuator(self.counter_prop_actuator, 0.0)
```

The single `thrust_calibration` constant captures everything that varies between drones (fan unit, ESC tuning, nozzle, line pressure). It is determined per-drone via the calibration procedure below and stored in the drone's legion config.

## Calibration procedure

Per-drone, on the bench, before the first flight with a wash payload:

```
1. Mount the drone on a fixed test rig (clamped to a bench, props OFF).
2. Place a kitchen scale or load cell horizontally against the wash nozzle.
3. Trigger wash_on(intensity=1.0). Read the reaction force on the scale (e.g. 530 g).
4. Trigger the counter-prop alone, ramping PWM until the scale reads ~0 g.
5. Record the counter-prop PWM value as the calibration at intensity=1.0.
6. Repeat at intensity=0.5 and 0.75 to verify the relationship is linear.
7. Compute thrust_calibration = (counter PWM at full) / 1.0  →  store in config.
```

In practice the calibration won't be perfect — water pressure drops as the tank empties, air temperature affects fan efficiency, the nozzle wears. PX4 absorbs the **~50–100 g residual** through normal tilt compensation, the same way it handles light wind. The counter-thrust just has to get the gross balance right; PX4 does the trim.

Re-calibrate after any change to the fan, ESC, valve, or nozzle.

## Wiring

```
Battery (4S 14.8 V)
    │
    ├── PDB ── ESC 1–4 ── lift motors (existing, unchanged)
    │
    ├── PM02 ── Pixhawk 6C (existing)
    │
    ├── ESC 5 ── counter-prop EDF (new)
    │    │
    │    └── PWM signal from Pixhawk AUX 6
    │
    └── solenoid valve (new, normally-closed, for water flow)
         │
         └── PWM / relay from Pixhawk AUX 5
```

Two new AUX outputs, both driven by `set_actuator` from legion. No relay or MOSFET on the AUX side — Pixhawk drives both directly. The solenoid valve takes 12 V battery power switched by its own integrated coil; the EDF takes 4S battery through a dedicated 30 A ESC.

## Weight budget

| Component | Paint drone | Wash drone |
|---|---|---|
| Frame + flight system | 800 g | 800 g |
| Spray can / water | 350 g | 500 g (water) |
| Servo / valve | 15 g (SG90) | 30 g (solenoid) |
| Counter-prop (EDF + ESC + bracket) | — | 100 g |
| Nozzle | 5 g (TeeJet) | 20 g (pressure nozzle) |
| Hose / fittings | — | 30 g |
| **Total payload** | **370 g** | **680 g** |
| **Drone all-up weight** | 1,170 g | 1,480 g |
| Available thrust (4× T-Motor 2216 880 KV with 10" props) | 3,200 g | 3,200 g |
| **Thrust-to-weight** | **2.7 : 1** | **2.2 : 1** |

2.2 : 1 is comfortably flyable. Less agile than the paint variant, but stable enough for the slow controlled passes that wash work consists of. No frame or motor changes are required to support the wash payload — only the swappable payload module and its two AUX outputs.

## One drone, swappable payload modules

The whole point of this design is that **paint and wash are not different drones**. They are different payload modules on the same drone. The frame, motors, ESCs, Pixhawk, Pi, and legion agent are identical:

```
PAINT MODULE                     WASH MODULE
┌───────────────────┐            ┌────────────────────┐
│ Spray can          │           │ Water cartridge     │
│ SG90 servo         │           │ Solenoid valve      │
│ Mount bracket      │           │ Pressure nozzle     │
│                    │           │ 64 mm EDF counter   │
│ ~370 g total       │           │ ~680 g total        │
└───────────────────┘            └────────────────────┘

       Same frame, same Pixhawk, same Pi, same legion agent.
       Legion config tells the drone which module is attached.
```

Legion config (per drone):

```yaml
# /etc/hivemind/legion.yaml on each drone's Pi
drone_id: drone-03
payload_module: wash         # "paint" or "wash"
actuators:
  primary: 5                 # AUX 5: servo (paint) or solenoid valve (wash)
  counter_prop: 6            # AUX 6: only used when payload_module = wash
wash:
  thrust_calibration: 0.82   # tuned per-drone via the calibration procedure
```

At startup, legion reads `payload_module` and instantiates either the paint controller (`PaintController` from [hw/nozzle](../nozzle/README.md)) or the `WashController` defined above. The rest of the legion agent — sortie executor, telemetry stream, local safety loop — is identical between the two.

This is the architectural payoff: **the slicer in oracle, the safety loops in legion, the RTK and ground station, the plan/apply lifecycle — none of it cares whether the drone is painting or washing**. The intent file format does need a per-region treatment spec to discriminate (a v1.1 schema addition), but everything else is shared.

## The remaining problem — water duration

Counter-thrust fixes the *force* problem. It does not fix the *capacity* problem:

```
Tank capacity                Flow rate           Sortie duration
──────────────               ──────────          ─────────────────
500 ml                       0.5 L/min (soft)    60 s
500 ml                       1.0 L/min           30 s
500 ml                       2.0 L/min           15 s

At 0.5 L/min, 0.3 m/s pass speed, 0.2 m spray width:
   60 s × 0.3 m/s × 0.2 m = 3.6 m² covered per sortie
```

3.6 m² per sortie at soft-wash pressure with a chemical cleaning solution is *not* terrible. A 10-drone fleet cycling through refills keeps several drones on the wall continuously, and soft washing is the primary commercial wash use case anyway (high-pressure blasting is for surface prep, which Hivemind explicitly doesn't address — see [main README → honest risks](../../README.md#honest-risks-and-caveats)).

The refill cycle for wash differs from paint mainly in *what* gets refilled: water cartridges + chemical concentrate, instead of paint cartridges. The refill station on the truck (see [hw/v2 → Ground station](../v2/README.md#ground-station-on-the-truck)) carries both, and the operator swaps cartridges by hand at the landing pad. ~15 s swap time, same as paint.

## Bill of materials — wash payload add-on

Per drone, on top of the v2 base flight platform:

| Item | Source | Price |
|---|---|---|
| 64 mm EDF ducted fan unit (motor + fan + duct) | Amazon.de "64 mm EDF 4S" | €15–20 |
| 30 A ESC for the EDF | Amazon.de | €8 |
| 12 V solenoid valve, normally-closed | Amazon.de | €5–8 |
| Adjustable pressure nozzle | Amazon.de | €5 |
| Water cartridge (500 ml, gravity or lightly pressurised) | Custom or repurposed | €10 |
| Silicone tubing + push-fit fittings | Amazon.de | €5 |
| EDF mount bracket | 3D-printed or aluminium | €3 |
| **Wash add-on per drone** | | **~€50–55** |

The wash payload is ~€50 on top of the existing paint drone. Prototype on a single drone, validate the calibration and the coverage rate, then decide whether the wash market is worth scaling to the full fleet.

## What this mechanism does *not* solve

Worth being explicit so the wash variant isn't oversold:

- **High-pressure blasting / surface prep.** Reaction force scales with pressure. A real high-pressure washer (e.g. 2,000 PSI) produces several kg of thrust — well past what a 64 mm EDF can counter on a quad this size. Hivemind wash is for **soft washing only**: low-pressure water + chemical cleaning agent, the kind of work that today is done by people with extension poles. Surface prep stays out of scope, same as it does for the paint variant.
- **Water containment / drip catch.** Wash water runs off the structure. For sensitive sites (over water, near drains, near landscaping) the same containment problem as overspray applies. Discussed in [main README → honest risks](../../README.md#honest-risks-and-caveats).
- **Variable nozzle types mid-mission.** One nozzle, one calibration, one wash chemistry per sortie. Switching nozzles requires re-calibration on the bench.
- **Wind. ** Soft-wash spray is even more wind-sensitive than paint spray (lower momentum droplets). Operating window is narrower than for the paint variant.

None of these block the wash variant from being commercially viable for soft-wash work. They just bound what it sells as.
