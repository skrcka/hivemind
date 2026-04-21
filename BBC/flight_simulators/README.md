# Flight Simulators for Custom Drone Development

A curated overview of open-source flight simulation tools, from physics-accurate SITL environments to lightweight dynamics libraries.

---

## 1. PX4 Autopilot + Gazebo — *Best overall recommendation*

[PX4](https://px4.io/) is a fully open-source professional flight controller used in research, industry, and custom drones. It handles all flight control logic.

**Custom airframe support:**
- Define exact motor positions, thrust directions, and mixing (quad, hex, X, H, or any custom layout).
- Set total weight (mass), inertia tensor, motor thrust curves, drag coefficients, etc.
- Tune PID controllers, filters, and all flight parameters.

**Gazebo physics simulator** lets you build a full 3D model of your drone in SDF format:
- Modify frame shape and size.
- Add motors with realistic thrust and motor dynamics.
- Set exact mass, center of gravity, and inertia.
- Test in any environment (wind, obstacles, complex terrain, etc.).

Supports **Software-In-The-Loop (SITL)** — run the exact same PX4 code as on a real drone, with zero hardware required.

**Quick start:**
1. [Install PX4](https://docs.px4.io/main/en/dev_setup/dev_env.html) (free and open-source).
2. Edit a Gazebo model file to match your frame geometry, motors, and weight.
3. Official docs: [PX4 Simulation](https://docs.px4.io/main/en/simulation/) · [Adding a new airframe](https://docs.px4.io/main/en/dev_airframes/adding_a_new_frame.html).

---

## 2. PX4 / ArduPilot + AirSim — *Great for realistic visuals & sensors*

[AirSim](https://github.com/microsoft/AirSim) (Microsoft, open-source, Unreal Engine) is excellent for custom drone development with high-fidelity visuals.

**You can define your own drone physics:**
- Mass, inertia, motor thrust, and torque.
- Custom 3D frame model.
- Run PX4 or ArduPilot inside it — the same firmware as on real hardware.

Particularly useful for testing cameras, computer vision pipelines, or any scenario requiring photo-realistic sensor simulation.

---

## 3. Pure Dynamics Simulators — *Fast iteration on weight / motors / frame*

Lightweight libraries ideal for control research and rapid parameter sweeps:

| Tool | Description |
|------|-------------|
| [RotorPy](https://github.com/spencerfolk/rotorpy) | Python-based; edit a parameter file with frame geometry, motor specs, weight, and inertia — run simulations instantly. |
| [Crazyflow](https://github.com/utiasDSL/crazyswarm2) | JAX-based, very fast; suited for swarm and learning experiments. |
| [RflySim / FlyEval](https://rflysim.com/) | Online multicopter performance calculator for quick hover time, payload, and speed estimates. |

---

## 4. Physical Open-Source Hardware — *When you're ready to build*

**3D-printable modular frames:**
- Search GitHub for `"open source 3D printed drone frame"` (e.g. MFPV modular FPV frame, F450-style customizable designs).
- Design your own frame in [FreeCAD](https://www.freecad.org/) or [OpenSCAD](https://openscad.org/) and print it.

**Pair it with:**
- [Pixhawk](https://pixhawk.org/) (or affordable clones) running PX4 firmware.
- Any brushless motors and ESCs.
- A fully open-source stack where you control weight, layout, and control algorithms.

Many university and research projects release complete open-source drone platforms (frame + motors + PX4) specifically to enable this kind of customization.
