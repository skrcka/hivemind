# Hivemind hardware — spray mechanism (nozzle, paint payload)

> Parent: [hw/README.md](../README.md) · Sibling payload: [hw/wash](../wash/README.md) (pressure-wash with counter-thrust)

This is the canonical build doc for the **paint payload**. v1 uses the **servo + standard aerosol spray can** approach described here. v2 will move to a peristaltic pump + bayonet cartridge for industrial coatings (not yet documented in detail — see [hw/v2](../v2/README.md)), but the v1 mechanism stays as the reference for cheap bench testing and prototyping any new payload design.

The paint payload is one of two swappable payload modules on the same drone platform — see [hw/wash](../wash/README.md) for the pressure-wash variant. Frame, Pixhawk, Pi, and legion agent are identical between the two; only the bottom-plate hardware and the AUX wiring differ.

## Concept in one sentence

Mount a normal hardware-store spray paint can to the drone, point the nozzle down, and have a servo arm push the nozzle button when oracle says "spray on."

```
STATE 1: OFF                    STATE 2: ON

servo arm UP                    servo arm DOWN
    ╲                               │
     ╲                              ▼
      ○ servo                       ○ servo

nozzle released                 nozzle pressed
no spray                        paint sprays
                                ▼▼▼▼▼▼
```

The spray can already has a spring-loaded nozzle. Push it down → paint sprays. Release → spring returns it up, spray stops. The servo just does the pushing. This is the entire mechanism. There is no pump, no relay, no MOSFET, no tubing, no priming, no clog risk.

### Why this approach for v1

| | Servo + spray can (this doc) | Peristaltic pump + bottle |
|---|---|---|
| Cost | ~€10 | ~€60 |
| Parts count | 5 | 8 |
| Wiring | 3 wires (servo to Pixhawk AUX) | Pump power, relay, GPIO, sensor |
| Priming required | No | Yes |
| Clog risk | None | Real (need to swap silicone tube) |
| Refill time | ~15 s (swap can) | ~30 s (swap bottle, prime) |
| Paint type | Whatever's in the can | Whatever the pump tolerates |
| Production-ready for industrial coatings | No | Yes |
| Right answer for v1 | **Yes** | No |

For v1 the entire point is to validate the *flight loop* — fly to a wall, trigger the payload, come back. The spray mechanism just needs to work reliably enough for bench tests and a handful of field trials. Aerosol cans win every dimension that matters here.

## Parts

| Item | Source | Price |
|---|---|---|
| SG90 servo (180° positional, 9 g) | dratek.cz, Amazon.de | €2–3 |
| Standard 400 ml aerosol spray paint can | Any hardware store | €3–5 |
| Hose clamp (60–80 mm diameter) | Hardware store | €1 |
| Small L-bracket (aluminium or 3D-printed) | Hardware store / DIY | €1–2 |
| M3 screws + nuts (4×) | Hardware store | €1 |
| **Total** | | **~€8–12** |

Cans are consumables — buy a box of 12 for a day of testing.

## Assembly

### Step 1 — Mount the spray can to the drone frame

```
Bottom of drone frame (top view)
┌──────────────────────────┐
│                          │
│   ○ motor    ○ motor     │
│                          │
│      ┌──────────┐        │
│      │ spray can│ ← hose clamp around can body,
│      │ (vertical,        │   tightened to a frame rail
│      │  nozzle down)     │   or bolted to bottom plate
│      └──────────┘        │
│                          │
│   ○ motor    ○ motor     │
│                          │
└──────────────────────────┘
```

Use a hose clamp (the adjustable metal band) around the spray can body. Tighten it to one of the X500's mounting rails or bolt it to the bottom plate. The can hangs vertically with the nozzle pointing down.

**Centering matters.** Mount the can as close to the drone's centre of gravity as possible. An off-centre 350 g mass will make the drone tilt. The X500 V2 has a centre mounting area between the landing gear legs that works well.

### Step 2 — Mount the servo

Screw or zip-tie the SG90 servo to a small L-bracket. The bracket attaches to the frame near the top of the spray can. The servo arm should reach the nozzle button when rotated.

```
Side view:

    ══════════════════  drone bottom plate
         │
    ┌────┴────┐
    │ bracket │ ← L-bracket bolted to frame
    │ ┌─────┐ │
    │ │servo│ │ ← SG90 screwed to bracket
    │ │  ╲  │ │
    │ └──╲──┘ │
    └────╲────┘
          ╲
           ▼  servo arm pushes down
    ┌─────●─────┐
    │   nozzle  │ ← spray can nozzle
    ├───────────┤
    │           │
    │  SPRAY    │
    │   CAN     │
    │  400 ml   │
    │           │
    └───────────┘
        ▼▼▼▼
       spray
```

**Servo arm length:** use the *shortest* arm from the SG90 kit (or cut one to ~10–15 mm). Shorter arm = more force on the nozzle. The SG90 outputs ~1.2 kg·cm of torque, so a 10 mm arm gives ~1.2 kgf at the tip — enough to fully depress a typical aerosol nozzle.

### Step 3 — Wire the servo

Three wires from the servo. Two options.

**Option A — to Pixhawk AUX (recommended):**

```
Servo wire          Pixhawk AUX port (any free one, e.g. AUX 5)
─────────           ──────────────────────────────────────────
Brown (GND)    →    GND pin
Red   (VCC)    →    5 V pin (servo rail)
Orange (SIG)   →    Signal pin
```

Pixhawk drives the servo directly through PX4's actuator output. No relay, no MOSFET, no extra controller.

**Option B — to Raspberry Pi GPIO (simpler for early bench testing):**

```
Brown (GND)    →    Pi GND  (pin 6)
Red   (VCC)    →    Pi 5 V  (pin 2)
Orange (SIG)   →    Pi GPIO 18 (pin 12)
```

Use Option B while you're still figuring out timing on the bench, then move to Option A once you're flying — Pixhawk-driven outputs are deterministic and survive Pi reboots.

## Software control

### Option A — From Pixhawk via MAVLink (recommended)

The legion agent on the Pi sends actuator commands through MAVSDK:

```python
# Inside the legion agent on the Raspberry Pi

async def spray_on(drone):
    """Push nozzle down — start spraying."""
    await drone.action.set_actuator(5, 1.0)   # AUX5, full position

async def spray_off(drone):
    """Release nozzle — stop spraying."""
    await drone.action.set_actuator(5, 0.0)   # AUX5, zero position

# During sortie execution:
for waypoint in sortie["waypoints"]:
    await fly_to(waypoint)

    if waypoint["spray"]:
        await spray_on(drone)
    else:
        await spray_off(drone)
```

PX4 config (set once via QGroundControl parameters):

```
AUX5 function = "Servo"   (or RC AUX5 passthrough for manual testing)
PWM_AUX_MIN5  = 1000
PWM_AUX_MAX5  = 2000
```

### Option B — Directly from Pi GPIO (simpler for testing)

```python
import pigpio
import time

pi = pigpio.pi()
SERVO_PIN = 18

def spray_on():
    pi.set_servo_pulsewidth(SERVO_PIN, 2000)  # 180° → push nozzle

def spray_off():
    pi.set_servo_pulsewidth(SERVO_PIN, 1000)  # 0° → release nozzle

# Bench test before flying:
spray_on()
time.sleep(3)   # spray for 3 seconds
spray_off()
```

## Testing procedure

Don't skip steps. Each builds confidence the next one is safe.

### Step 1 — Bench test (no flying)

```
1. Mount can + servo on a piece of wood
2. Wire servo to Pi or Pixhawk
3. Run spray_on()  → verify can sprays
4. Run spray_off() → verify can stops
5. Adjust servo arm position if it doesn't fully press
   or doesn't fully release the nozzle
6. Run 50 on/off cycles to verify reliability
```

### Step 2 — Ground test on drone (props off)

```
1. Mount the full assembly on the drone
2. Power up Pixhawk + Pi (battery connected, props OFF)
3. Trigger spray on/off from the laptop via SSH
4. Check that the weight is centred — drone shouldn't tip on its skids
5. If it tips, adjust can position or add counterweight
```

### Step 3 — Hover test (no spray)

```
1. Fly the drone with can mounted but EMPTY (use a dead can)
2. Check flight stability with the extra weight
3. If PX4 oscillates, retune PID (QGroundControl → Vehicle Setup → PID Tuning)
4. Practice hovering at 2–3 m height
```

### Step 4 — Spray test

```
1. Full can mounted, fly to 2 m height
2. Hover over a cardboard target on the ground
3. Trigger spray_on() for 2 seconds
4. Check the spray pattern on the cardboard
5. Adjust height for desired coverage width
6. Repeat at different heights: 1 m, 1.5 m, 2 m, 3 m
```

Log the standoff distance ↔ pattern width relationship — that table is the input the planner needs to lay out lanes in v2.

## Refill = can swap

There is no refill mechanism. When the can is empty:

```
1. Drone lands on pad
2. Operator loosens hose clamp        (~5 s)
3. Pulls empty can out
4. Slides new full can in
5. Tightens hose clamp                (~5 s)
6. Drone takes off
```

Total swap time: **~15 s**.

## Specs summary

| Parameter | Value |
|---|---|
| Spray medium | Standard 400 ml aerosol spray paint |
| Actuation | SG90 servo, 180° positional, 9 g |
| Control | Pixhawk AUX PWM (preferred) or Pi GPIO |
| Force on nozzle | ~1.2 kgf (10 mm arm at 4.8 V) |
| Response time | ~0.12 s full travel |
| Mechanism weight | ~20 g (servo + bracket + clamp) |
| Full can weight | ~350 g |
| Empty can weight | ~100 g |
| Power draw while pressing | ~100 mA from Pixhawk 5 V rail |
| Can swap time | ~15 s |
| Cost (mechanism only) | ~€10 |

## What this mechanism does *not* do

Worth being explicit so v1 isn't oversold:

- **No flow rate control.** Spray is on or off. The cardboard test gives you a single calibrated pattern at a single standoff. This is fine for v1 because plans can be expressed as "spray here for N seconds at standoff X." Industrial coating thickness requires real flow control — that's a v2 problem.
- **No real industrial coatings.** Aerosol cans hold ~400 ml of consumer paint. Real bridge coatings are zinc-rich primers, epoxy intermediates, polyurethane topcoats — none of which come in aerosol form, all of which need a proper pump. v2 will replace this entire mechanism with the peristaltic-pump + bayonet-cartridge approach in [hw/v2](../v2/README.md).
- **No paint level sensing.** The operator counts seconds of spray time and swaps the can when in doubt. v2 adds an HX711 load cell.
- **No clean-up between paint changes.** Different colour or paint type → use a new can.

Everything in this list is on the v2 path. None of it blocks v1 from doing useful work.
