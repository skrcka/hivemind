# HIVEMIND — Investment & Development Budget

**Industrial Drone Swarm Platform**
Confidential — April 2026

---

## Executive Summary

HIVEMIND is an autonomous drone swarm platform for industrial surface maintenance — painting, cleaning, and inspection of bridges and large structures. The system eliminates scaffolding (40-50% of traditional project costs), reduces project timelines from months to days, and removes workers from dangerous heights.

This document outlines the development budget required to take HIVEMIND from concept to a commercially operational system deployed from a single van/truck, including all R&D, regulatory certification, hardware, and initial operations.

---

## Total Investment Required

| Category | Budget |
|---|---|
| Phase 1: R&D and Prototyping (Months 1-6) | €73,585 |
| Phase 2: Software Development (Months 3-12) | €198,200 |
| Phase 3: Regulatory & Certification (Months 4-12) | €18,500 |
| Phase 4: Production Fleet & Van Build (Months 10-14) | €38,500 |
| Phase 5: Testing & Validation (Months 12-16) | €14,000 |
| Contingency (15%) | €51,400 |
| **TOTAL INVESTMENT** | **€394,185** |

Total development timeline: 16 months from start to first commercial deployment. Budget assumes a core team of 4 engineers throughout development.

---

## Team Structure

| Role | Focus | Monthly Salary | Months Active |
|---|---|---|---|
| Lead Engineer / CTO | Architecture, drone HW, embedded Rust, system integration | €7,000 | 1-16 (16 months) |
| Backend Engineer | Oracle (Rust), fleet scheduling, mission slicing, comms protocol | €6,000 | 1-16 (16 months) |
| Embedded Engineer | Legion agent (Rust), MAVLink, PX4 integration, sensor fusion, spray mechanism | €6,000 | 1-14 (14 months) |
| Frontend Engineer | Pantheon UI (Tauri + React + Three.js), 3D visualization, operator UX | €5,500 | 4-16 (13 months) |

---

## Phase 1: R&D and Prototyping

**Months 1-6.** Build and test prototype drones, validate spray mechanism, prove swarm coordination with 2-3 drones.

### Hardware Prototyping

| Item | Qty | Unit Cost | Total |
|---|---|---|---|
| Holybro S500 V2 Dev Kit (Pixhawk 6C) | 3 | €500 | €1,500 |
| Raspberry Pi 5 (4GB) | 3 | €80 | €240 |
| 4S 3000mAh LiPo batteries | 12 | €30 | €360 |
| Battery charger (6-port) | 1 | €150 | €150 |
| RTK GPS modules (H-RTK F9P) | 3 | €200 | €600 |
| RTK Base Station (ArduSimple) | 1 | €400 | €400 |
| SiK telemetry radios 433MHz | 3 pairs | €35 | €105 |
| SG90 servos + spray mounting hardware | 10 | €5 | €50 |
| Spray cans (testing supply) | 50 | €4 | €200 |
| Solenoid valves, nozzles, tubing, fittings | 5 sets | €25 | €125 |
| 64mm EDF counter-prop kit (wash module) | 2 | €25 | €50 |
| Distance sensors (TFMini-S) | 3 | €25 | €75 |
| Xbox controller + USB adapters | 2 | €15 | €30 |
| Carbon fiber tube, brackets, 3D printing filament | misc | — | €200 |
| Spare parts (motors, ESCs, props, connectors) | misc | — | €500 |
| **Subtotal: Hardware Prototyping** | | | **€4,585** |

### Scout Drone (Vanguard)

| Item | Qty | Unit Cost | Total |
|---|---|---|---|
| DJI Air 3 (or equivalent camera drone) | 1 | €900 | €900 |
| Extra batteries + accessories | 1 | €200 | €200 |
| **Subtotal: Scout Drone** | | | **€1,100** |

### Development Workstation & Tools

| Item | Qty | Unit Cost | Total |
|---|---|---|---|
| Rugged tablet (Getac F110 / Dell Latitude 7230) | 1 | €1,000 | €1,000 |
| Development laptops (if needed) | 2 | €1,500 | €3,000 |
| PX4 SITL simulation setup | 1 | €0 | €0 |
| Testing field rental / insurance | 6 mo | €200/mo | €1,200 |
| 3D printer (Prusa MK4 or similar) | 1 | €700 | €700 |
| Misc tools (soldering station, multimeter, hex set) | 1 set | €300 | €300 |
| **Subtotal: Workstation & Tools** | | | **€6,200** |

### Personnel (Phase 1)

| Role | Duration | Monthly Cost | Total |
|---|---|---|---|
| Lead Engineer / CTO | 6 months | €7,000 | €42,000 |
| Backend Engineer | 6 months | €6,000 | €36,000 |
| Embedded Engineer | 6 months | €6,000 | €36,000 |
| Frontend Engineer (starts Month 4) | 3 months | €5,500 | €16,500 |
| **Subtotal: Personnel Phase 1** | | | **€130,500** |

Note: Phase 1 and Phase 2 personnel overlap. To avoid double-counting, personnel costs are split proportionally. Phase 1 covers Months 1-6 salaries. Phase 2 covers Months 7-16 salaries.

| | |
|---|---|
| **PHASE 1 TOTAL** | **€142,385** |

---

## Phase 2: Software Development

**Months 7-16 (personnel continuation).** Build the full software stack: Oracle (mission slicer + fleet scheduler), Legion agent (drone-side execution), and Pantheon (operator UI). Core architecture work begins in Phase 1 but the majority of software effort is in this phase.

### Software Components

| Component | Technology | Effort | Description |
|---|---|---|---|
| Legion Agent | Rust + rust-mavlink | 3 months | Drone-side execution engine. Receives sorties, controls PX4 via MAVLink, manages spray actuator, local safety loops, telemetry reporting. Cross-compiles to Pi and ESP32. |
| Oracle Core | Rust (prototyped in Python) | 5 months | Mission slicer (mesh → spray passes), sortie generator, fleet scheduler, dynamic rebalancer, plan/apply lifecycle, step confirmation protocol. |
| Oracle Comms | Rust + tokio + WebSocket | 1 month | Fleet communication server. WebSocket hub for all legion agents. Heartbeat monitoring, conflict detection, radio loss handling. |
| Pantheon v1 | Blender + Skybrush Studio | 1 month | Initial operator UI using existing open-source tools. 3D mesh import, trajectory planning, mission export. |
| Pantheon v2 | Tauri + React + Three.js | 4 months | Custom operator UI. 3D bridge viewer, region painter, plan review/approve, live fleet dashboard, sortie timeline scrubber. |
| 3D Scan Pipeline | OpenDroneMap + Python | 1 month | Automated processing of scout drone photos into georeferenced 3D mesh. GCP alignment, coordinate transform tools. |

### Personnel (Phase 2 — Months 7-16)

| Role | Duration | Monthly Cost | Total |
|---|---|---|---|
| Lead Engineer / CTO | 10 months | €7,000 | €70,000 |
| Backend Engineer | 10 months | €6,000 | €60,000 |
| Embedded Engineer | 8 months (ends Month 14) | €6,000 | €48,000 |
| Frontend Engineer | 10 months | €5,500 | €55,000 |
| Software licenses & cloud services | 10 months | €100 | €1,000 |
| **Subtotal: Personnel Phase 2** | | | **€234,000** |

### Open Source Software (No Cost)

| Software | License | Purpose |
|---|---|---|
| PX4 Autopilot | BSD | Flight firmware on all drones |
| rust-mavlink | MIT | MAVLink communication library |
| Skybrush Suite (Server, Live, Studio) | GPL | Swarm management, GCS, mission planning |
| QGroundControl | Apache 2.0 | Ground control station |
| OpenDroneMap | AGPL | 3D reconstruction from drone photos |
| Blender | GPL | 3D visualization and mission planning |
| Three.js / React Three Fiber | MIT | 3D web rendering for Pantheon UI |

| | |
|---|---|
| **PHASE 2 TOTAL** | **€235,000** |

---

## Phase 3: Regulatory & Certification

**Months 4-12.** Obtain EASA Specific Category authorization via SORA 2.5 risk assessment, filed with the Czech Civil Aviation Authority (ÚCL). Runs in parallel with development.

| Item | Details | Total |
|---|---|---|
| SORA 2.5 Risk Assessment preparation | ConOps document, ground risk assessment, air risk assessment, OSO compliance evidence | €5,000 |
| Aviation safety consultant | Review SORA application, advise on mitigations, liaise with ÚCL. ~40 hours at €125/hr | €5,000 |
| ÚCL application fees | Operational authorization filing | €500 |
| Pilot certification (A1/A3 + A2 + Specific) | Training and examination for 2 operators | €1,000 |
| Liability insurance (drone fleet, commercial) | Annual premium for 10 drones, commercial operations | €3,000 |
| Legal fees (contracts, T&Cs, liability) | Lawyer review of commercial service agreements | €2,000 |
| Environmental compliance review | Paint overspray containment, water discharge permits | €2,000 |

Note: SORA authorization timeline is typically 4-12 months from submission. Early engagement with ÚCL is critical. The consultant cost covers pre-submission review to minimize rejection risk.

| | |
|---|---|
| **PHASE 3 TOTAL** | **€18,500** |

---

## Phase 4: Production Fleet & Van Build

**Months 10-14.** Build the production fleet of 10 drones and outfit a van/truck as the mobile command and refill station.

### Production Drone Fleet (10 units)

| Item | Per Drone | x10 Total |
|---|---|---|
| Custom frame (500mm carbon fiber, optimized for payload) | €100 | €1,000 |
| Pixhawk 6C Mini + PM02 | €230 | €2,300 |
| H-RTK F9P GPS (RTK-capable) | €200 | €2,000 |
| Raspberry Pi 5 (or ESP32 if legion supports it) | €80 | €800 |
| Motors 4x + ESCs 4x + PDB | €100 | €1,000 |
| Propellers (10 sets per drone) | €30 | €300 |
| WiFi adapter (external antenna) | €20 | €200 |
| Spray system (valve + nozzle + boom + sensor) | €40 | €400 |
| Wiring, connectors, mounting hardware | €20 | €200 |
| Assembly and testing labor | €50 | €500 |
| **Subtotal: 10 Production Drones** | **€870** | **€8,700** |

### Battery Pool

| Item | Qty | Unit Cost | Total |
|---|---|---|---|
| 4S 3000mAh LiPo batteries | 30 | €30 | €900 |
| 6-port parallel charger | 2 | €150 | €300 |
| Battery storage/transport case (fireproof) | 2 | €100 | €200 |
| **Subtotal: Batteries** | | | **€1,400** |

### Van / Truck Outfitting

| Item | Qty | Unit Cost | Total |
|---|---|---|---|
| Used van (VW Transporter / Renault Master) | 1 | €15,000 | €15,000 |
| RTK base station (permanent roof mount) | 1 | €400 | €400 |
| Dedicated WiFi mesh router (outdoor rated) | 1 | €200 | €200 |
| 12V compressor (for pressurized cartridges) | 1 | €150 | €150 |
| Paint refill station (pump, fittings, scale, containment) | 1 | €400 | €400 |
| Power system (inverter 1500W + wiring) | 1 | €200 | €200 |
| Operator workstation (NUC + monitor mount + tablet mount) | 1 | €900 | €900 |
| Landing pad system (10 pads + storage) | 1 | €150 | €150 |
| Drone storage rack (in van) | 1 | €300 | €300 |
| Paint storage (containment, shelving) | 1 | €200 | €200 |
| Signage, safety equipment, fire extinguisher | 1 | €200 | €200 |
| Generator (backup power, 2kW) | 1 | €500 | €500 |
| **Subtotal: Van Outfitting** | | | **€18,600** |

### Scout Drone (Production)

| Item | Qty | Unit Cost | Total |
|---|---|---|---|
| RTK-capable survey drone (DJI Mavic 3 Enterprise) | 1 | €3,500 | €3,500 |
| Extra batteries + accessories | 1 | €500 | €500 |
| **Subtotal: Production Scout** | | | **€4,000** |

| | |
|---|---|
| **PHASE 4 TOTAL** | **€38,500** |

---

## Phase 5: Testing & Validation

**Months 12-16.** Full system integration testing, real-world bridge trials, coating quality validation, and commercial readiness.

| Item | Details | Total |
|---|---|---|
| Controlled test site rental | Industrial facility or bridge section, 4 multi-day sessions | €2,000 |
| Paint materials for testing | Industrial anti-corrosion paint, primers, thinners, 200L | €2,000 |
| Coating quality testing equipment | Wet/dry film thickness gauge, adhesion tester, gloss meter | €1,500 |
| Independent coating lab analysis | Third-party verification coating quality meets standards | €2,000 |
| Travel and subsistence | Team travel to test sites, 4 trips | €2,000 |
| Video documentation / marketing | Professional footage of system in operation for demos | €2,500 |
| Consumables (props, batteries, nozzles, cans) | Wear items during intensive testing period | €2,000 |

| | |
|---|---|
| **PHASE 5 TOTAL** | **€14,000** |

---

## Investment Summary

| Phase | Timeline | Budget |
|---|---|---|
| Phase 1: R&D and Prototyping | Months 1-6 | €142,385 |
| Phase 2: Software Development | Months 7-16 | €235,000 |
| Phase 3: Regulatory & Certification | Months 4-12 | €18,500 |
| Phase 4: Production Fleet & Van Build | Months 10-14 | €38,500 |
| Phase 5: Testing & Validation | Months 12-16 | €14,000 |
| **Subtotal: Development** | | **€448,385** |
| **Contingency (15%)** | | **€67,258** |
| **TOTAL INVESTMENT REQUIRED** | | **€515,643** |

---

## Use of Funds

| Category | Amount | % of Total |
|---|---|---|
| Personnel (engineering salaries) | €364,500 | 71% |
| Hardware (drones, van, equipment) | €44,785 | 9% |
| Regulatory & legal | €18,500 | 3% |
| Testing & validation | €14,000 | 3% |
| Tools & workstations | €6,200 | 1% |
| Contingency | €67,258 | 13% |
| **TOTAL** | **€515,643** | **100%** |

---

## Revenue Potential

| Metric | Value |
|---|---|
| Traditional bridge painting cost | €130-270 / m² |
| HIVEMIND drone painting cost | €20-45 / m² |
| Cost reduction for client | 70-85% |
| Average bridge surface area | 5,000 m² |
| Revenue per bridge (at €80/m² client price) | €400,000 |
| Cost per bridge (HIVEMIND operations) | €100,000-225,000 |
| Gross margin per bridge | €175,000-300,000 (44-75%) |
| Bridges per year (1 van, conservative) | 8-12 |
| Annual revenue (year 1) | €3.2M - €4.8M |
| Annual gross profit (year 1) | €1.4M - €3.6M |
| **Payback period on €516K investment** | **< 2-3 months of operations** |

---

## Key Milestones

| Month | Milestone | Deliverable |
|---|---|---|
| Month 2 | First flight with spray mechanism | Drone sprays paint on a test wall |
| Month 4 | Multi-drone coordination | 2-3 drones painting simultaneously |
| Month 6 | Prototype complete | Full system demo: scan, plan, spray |
| Month 8 | Oracle v1 complete | Automated mission slicing and fleet scheduling |
| Month 10 | SORA submitted | Regulatory application filed with ÚCL |
| Month 12 | Production fleet built | 10 drones + van ready for field testing |
| Month 14 | SORA approved (target) | Legal authorization for commercial operations |
| Month 16 | First commercial project | Paying customer bridge painting job |

---

## Market Opportunity

The EU has approximately 500,000 road bridges requiring regular maintenance painting on 10-15 year cycles. Even capturing 0.1% of this market represents 500 bridges per year. At €400K revenue per bridge, the addressable market for HIVEMIND in Europe alone exceeds €200M annually.

HIVEMIND's competitive advantage is threefold: elimination of scaffolding (40-50% cost savings), dramatic timeline reduction (days vs months), and zero workers at height (safety and insurance benefits). No competitor currently offers swarm-based industrial painting — existing drone painting companies operate single tethered drones manually.

---

## Technology Stack

| Component | Technology | Status |
|---|---|---|
| Flight firmware | PX4 (open source, BSD) | Existing — no development needed |
| Flight controller | Pixhawk 6C (open hardware) | Off-the-shelf |
| Drone-side agent (Legion) | Rust + rust-mavlink | Custom development |
| Fleet orchestrator (Oracle) | Rust + tokio + axum | Custom development |
| Operator UI (Pantheon) | Tauri + React + Three.js | Custom development |
| Mission planning | Blender + Skybrush Studio (GPL) | Existing + custom plugins |
| Ground control | QGroundControl (Apache 2.0) | Existing — no development needed |
| 3D reconstruction | OpenDroneMap (AGPL) | Existing — integration only |
| Communication | MAVLink (drone) + WebSocket (fleet) | Standard protocols |

---

## Risk Factors

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| SORA approval delayed | Medium | High | Early ÚCL engagement, safety consultant, conservative ConOps |
| Coating quality insufficient | Medium | High | Industrial paint supplier partnership, independent lab testing in Phase 5 |
| Wind limitations reduce operational days | Medium | Medium | Target sheltered bridge faces first, develop wind tolerance data |
| Hardware failure rate too high | Low | Medium | Redundant drone fleet (10 units), extensive testing in Phase 5 |
| Competitor enters market | Low | Medium | First-mover advantage, patent key innovations, build customer relationships |
| Regulatory environment changes | Low | High | Monitor EASA developments, build LUC certification for operational flexibility |

---

*Confidential — HIVEMIND Project — April 2026*