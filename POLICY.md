# Project Policy

## Autopilot / firmware (PX4 only)

This repository targets **PX4** as the only flight firmware for all drones.

Due to licensing constraints, this project must **not** include or depend on **ArduPilot**.

Contributors must not:
- add ArduPilot source code, headers, or copied snippets
- add ArduPilot git submodules or vendor tarballs
- check in ArduPilot build outputs (binaries, generated files, parameter dumps, etc.)
- add documentation or tooling that assumes ArduPilot is a supported runtime target

Allowed / encouraged:
- MAVLink / MAVSDK integrations that work with PX4
- PX4 SITL / Gazebo / AirSim workflows

If you are unsure whether a dependency or file would be considered "ArduPilot-derived", do not add it without an explicit maintainer decision.

