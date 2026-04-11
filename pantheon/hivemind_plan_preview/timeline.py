"""Time → drone-position interpolation for plan visualization.

Given a parsed Plan with sorties and steps, compute each drone's position at
any wall-clock time `t`. The position comes out in the same Cartesian frame
the caller's `convert` callback projects waypoints into — typically local
ENU metres around a chosen GPS origin (see `coords.py`).

Has NO Blender dependency. Pure functional, easy to unit-test.
"""

from __future__ import annotations

import math
from collections.abc import Callable
from dataclasses import dataclass

from .plan_loader import Sortie, SortieStep, Waypoint


@dataclass(frozen=True)
class Pose3:
    """A 3D position used for animation keyframes (already in target frame)."""

    x: float
    y: float
    z: float


WaypointConverter = Callable[[Waypoint], Pose3]


def step_path(step: SortieStep) -> list[Waypoint]:
    """Return the ordered list of waypoints the drone visits in this step.

    For path-following steps (`SprayPass`, multi-waypoint `Transit`), the
    `path` is the trajectory. For single-waypoint steps it is just the
    destination.
    """
    if step.path:
        return list(step.path)
    return [step.waypoint]


def sortie_position_at(
    sortie: Sortie,
    t: float,
    initial_position: Waypoint,
    convert: WaypointConverter,
) -> Pose3:
    """Position of a drone executing this sortie at time `t`.

    `t` is wall-clock seconds from plan start (NOT from sortie start).
    `initial_position` is where the drone parks before its first sortie.
    """
    # Before sortie starts: parked at the initial position
    if t <= sortie.start_time_s:
        return convert(initial_position)

    sortie_end = sortie.start_time_s + sortie.expected_duration_s
    # After sortie ends: at the final waypoint of the final step
    if t >= sortie_end:
        return convert(_last_waypoint(sortie))

    # Within the sortie: walk steps until we find the active one
    elapsed_in_sortie = t - sortie.start_time_s
    cumulative = 0.0
    prev_endpoint = initial_position

    for step in sortie.steps:
        step_dur = step.expected_duration_s
        if elapsed_in_sortie <= cumulative + step_dur:
            local_t = elapsed_in_sortie - cumulative
            return _interpolate_step(step, prev_endpoint, local_t, step_dur, convert)
        cumulative += step_dur
        prev_endpoint = step_path(step)[-1]

    # Fallback (numerical edge case): final waypoint
    return convert(_last_waypoint(sortie))


def _last_waypoint(sortie: Sortie) -> Waypoint:
    if not sortie.steps:
        raise ValueError(f"Sortie {sortie.sortie_id!r} has no steps")
    return step_path(sortie.steps[-1])[-1]


def _interpolate_step(
    step: SortieStep,
    prev_endpoint: Waypoint,
    local_t: float,
    duration: float,
    convert: WaypointConverter,
) -> Pose3:
    """Position within a single step at relative time `local_t` of `duration`.

    Time is distributed across path segments proportionally to **distance**,
    not to segment count — so a long initial transit segment doesn't get
    crammed into the same time slice as a short final segment.
    """
    waypoints = [prev_endpoint, *step_path(step)]
    if duration <= 0 or len(waypoints) < 2:
        return convert(waypoints[-1])

    poses = [convert(w) for w in waypoints]
    seg_lengths = [_distance(poses[i], poses[i + 1]) for i in range(len(poses) - 1)]
    total_length = sum(seg_lengths)

    if total_length <= 0.0:
        # All waypoints sit at the same point — nothing to interpolate
        return poses[-1]

    fraction = max(0.0, min(1.0, local_t / duration))
    target_distance = fraction * total_length

    accumulated = 0.0
    for i, seg_len in enumerate(seg_lengths):
        if accumulated + seg_len >= target_distance:
            seg_t = (target_distance - accumulated) / seg_len if seg_len > 0 else 0.0
            return _lerp(poses[i], poses[i + 1], seg_t)
        accumulated += seg_len

    return poses[-1]


def _lerp(a: Pose3, b: Pose3, t: float) -> Pose3:
    return Pose3(
        x=a.x + (b.x - a.x) * t,
        y=a.y + (b.y - a.y) * t,
        z=a.z + (b.z - a.z) * t,
    )


def _distance(a: Pose3, b: Pose3) -> float:
    return math.sqrt((a.x - b.x) ** 2 + (a.y - b.y) ** 2 + (a.z - b.z) ** 2)
