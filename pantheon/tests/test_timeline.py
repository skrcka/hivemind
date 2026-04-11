"""Unit tests for hivemind_plan_preview.timeline."""

from __future__ import annotations

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from hivemind_plan_preview.plan_loader import Sortie, SortieStep, Waypoint
from hivemind_plan_preview.timeline import (
    Pose3,
    sortie_position_at,
    step_path,
)


def _identity(wp: Waypoint) -> Pose3:
    """Pretend lat/lon/alt are just metres in a flat 3D frame.

    Lets us write tests over the timeline arithmetic without depending on
    coords.gps_to_enu — those have their own tests.
    """
    return Pose3(x=wp.lon, y=wp.lat, z=wp.alt_m)


def _at(sortie: Sortie, t: float, initial: Waypoint) -> Pose3:
    """Shorter helper for the tests below."""
    return sortie_position_at(sortie, t=t, initial_position=initial, convert=_identity)


def _step(
    index: int,
    *,
    waypoint: Waypoint,
    path: tuple[Waypoint, ...] | None = None,
    duration: float = 10.0,
) -> SortieStep:
    return SortieStep(
        index=index,
        step_type="Transit",
        waypoint=waypoint,
        path=path,
        speed_m_s=5.0,
        expected_duration_s=duration,
    )


def _wp(lat: float, lon: float, alt: float = 0.0) -> Waypoint:
    return Waypoint(lat=lat, lon=lon, alt_m=alt)


class TestStepPath(unittest.TestCase):
    def test_single_waypoint_step(self) -> None:
        s = _step(0, waypoint=_wp(0.0, 1.0))
        self.assertEqual(step_path(s), [_wp(0.0, 1.0)])

    def test_path_following_step(self) -> None:
        path = (_wp(0.0, 0.0), _wp(0.0, 1.0), _wp(0.0, 2.0))
        s = _step(0, waypoint=_wp(0.0, 2.0), path=path)
        self.assertEqual(step_path(s), list(path))


class TestSortiePositionAt(unittest.TestCase):
    def setUp(self) -> None:
        self.initial = _wp(0.0, 0.0, 0.0)
        self.steps = (
            _step(0, waypoint=_wp(0.0, 10.0, 0.0), duration=10.0),
            _step(1, waypoint=_wp(0.0, 20.0, 0.0), duration=10.0),
        )
        self.sortie = Sortie(
            sortie_id="s1",
            drone_id="d1",
            steps=self.steps,
            expected_duration_s=20.0,
            start_time_s=0.0,
        )

    def test_before_start_returns_initial(self) -> None:
        pose = _at(self.sortie, -5.0, self.initial)
        self.assertEqual(pose, _identity(self.initial))

    def test_at_start_returns_initial(self) -> None:
        pose = _at(self.sortie, 0.0, self.initial)
        self.assertEqual(pose, _identity(self.initial))

    def test_after_end_returns_final_waypoint(self) -> None:
        pose = _at(self.sortie, 100.0, self.initial)
        self.assertEqual(pose, _identity(self.steps[1].waypoint))

    def test_midpoint_of_first_step(self) -> None:
        # 5s into the sortie, halfway through the first 10s step that goes
        # from (0,0) -> (0,10): expect lon=5
        pose = _at(self.sortie, 5.0, self.initial)
        self.assertAlmostEqual(pose.x, 5.0)
        self.assertAlmostEqual(pose.y, 0.0)

    def test_midpoint_of_second_step(self) -> None:
        # 15s in: halfway through the second step that goes (0,10) -> (0,20)
        pose = _at(self.sortie, 15.0, self.initial)
        self.assertAlmostEqual(pose.x, 15.0)
        self.assertAlmostEqual(pose.y, 0.0)

    def test_step_boundary(self) -> None:
        # Exactly at t=10s: should be at the endpoint of the first step
        pose = _at(self.sortie, 10.0, self.initial)
        self.assertAlmostEqual(pose.x, 10.0)
        self.assertAlmostEqual(pose.y, 0.0)

    def test_offset_start_time(self) -> None:
        # Same sortie, but starts at t=100. At t=105 we should be at the
        # midpoint of the first step.
        offset_sortie = Sortie(
            sortie_id="s1",
            drone_id="d1",
            steps=self.steps,
            expected_duration_s=20.0,
            start_time_s=100.0,
        )
        pose = _at(offset_sortie, 105.0, self.initial)
        self.assertAlmostEqual(pose.x, 5.0)


class TestPathFollowingInterpolation(unittest.TestCase):
    def test_distance_proportional(self) -> None:
        # A step with two segments of different lengths.
        # Initial → wp1: distance 10
        # wp1 → wp2:    distance 30
        # Total length 40, total duration 40 → 1 m/s effective.
        # At t=15, target distance = 15.
        # First segment exhausts at distance 10 (t=10), so at t=15 we're 5
        # into the second segment → 5/30 of the way from wp1 to wp2.
        initial = _wp(0.0, 0.0, 0.0)
        wp1 = _wp(0.0, 10.0, 0.0)
        wp2 = _wp(0.0, 40.0, 0.0)
        path = (wp1, wp2)
        step = _step(0, waypoint=wp2, path=path, duration=40.0)

        sortie = Sortie(
            sortie_id="s",
            drone_id="d",
            steps=(step,),
            expected_duration_s=40.0,
            start_time_s=0.0,
        )
        pose = _at(sortie, 15.0, initial)
        # We should be 5 units past wp1 (at lon=15)
        self.assertAlmostEqual(pose.x, 15.0)


class TestEdgeCases(unittest.TestCase):
    def test_zero_duration_step_does_not_divide_by_zero(self) -> None:
        wp_a = _wp(0.0, 0.0)
        wp_b = _wp(0.0, 5.0)
        step = _step(0, waypoint=wp_b, duration=0.0)
        sortie = Sortie(
            sortie_id="s",
            drone_id="d",
            steps=(step,),
            expected_duration_s=10.0,
            start_time_s=0.0,
        )
        # Should not crash
        pose = _at(sortie, 5.0, wp_a)
        self.assertIsInstance(pose, Pose3)

    def test_coincident_waypoints(self) -> None:
        wp = _wp(1.0, 2.0, 3.0)
        step = _step(0, waypoint=wp, duration=10.0)
        sortie = Sortie(
            sortie_id="s",
            drone_id="d",
            steps=(step,),
            expected_duration_s=10.0,
            start_time_s=0.0,
        )
        pose = _at(sortie, 5.0, wp)
        # Should just return the waypoint, not crash on zero distance
        self.assertEqual(pose, _identity(wp))


if __name__ == "__main__":
    unittest.main()
