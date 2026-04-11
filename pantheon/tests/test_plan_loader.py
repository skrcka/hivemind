"""Unit tests for hivemind_plan_preview.plan_loader."""

from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from hivemind_plan_preview.plan_loader import (
    Intent,
    Plan,
    Sortie,
    Triangle,
    Vertex,
    Waypoint,
    load_intent,
    load_plan,
    parse_intent,
    parse_plan,
)


def _intent_dict() -> dict:
    return {
        "version": "1.0",
        "scan": {
            "id": "bridge_test",
            "source_file": "/tmp/bridge.obj",
            "georeferenced": True,
        },
        "regions": [
            {
                "id": "north_face",
                "name": "North face",
                "area_m2": 12.5,
                "faces": [
                    {
                        "vertices": [
                            [0.0, 0.0, 0.0],
                            [1.0, 0.0, 0.0],
                            [0.0, 1.0, 0.0],
                        ],
                        "normal": [0.0, 0.0, 1.0],
                    }
                ],
            }
        ],
    }


def _plan_dict(intent_embedded: bool = True, n_steps: int = 2) -> dict:
    plan: dict = {
        "id": "plan_001",
        "sorties": [
            {
                "sortie_id": "sortie_001",
                "drone_id": "drone-01",
                "expected_duration_s": 60.0,
                "steps": [
                    {
                        "index": i,
                        "step_type": "Transit",
                        "waypoint": {
                            "lat": 50.0 + i * 0.0001,
                            "lon": 14.0 + i * 0.0001,
                            "alt_m": 50.0,
                        },
                        "speed_m_s": 5.0,
                        "expected_duration_s": 30.0,
                        "spray": False,
                    }
                    for i in range(n_steps)
                ],
            }
        ],
        "schedule": {"total_duration_s": 60.0},
    }
    if intent_embedded:
        plan["intent"] = _intent_dict()
    return plan


# ─── Intent ─────────────────────────────────────────────────────────


class TestParseIntent(unittest.TestCase):
    def test_minimal_fields(self) -> None:
        intent = parse_intent(_intent_dict())
        self.assertIsInstance(intent, Intent)
        self.assertEqual(intent.version, "1.0")
        self.assertEqual(intent.scan.id, "bridge_test")
        self.assertTrue(intent.scan.georeferenced)
        self.assertEqual(len(intent.regions), 1)

    def test_region_geometry(self) -> None:
        intent = parse_intent(_intent_dict())
        region = intent.regions[0]
        self.assertEqual(region.id, "north_face")
        self.assertEqual(len(region.triangles), 1)
        tri = region.triangles[0]
        self.assertIsInstance(tri, Triangle)
        self.assertEqual(len(tri.vertices), 3)
        self.assertEqual(tri.vertices[0], Vertex(0.0, 0.0, 0.0))
        self.assertEqual(tri.normal, (0.0, 0.0, 1.0))

    def test_triangle_must_have_three_vertices(self) -> None:
        bad = _intent_dict()
        bad["regions"][0]["faces"][0]["vertices"] = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
        ]
        with self.assertRaises(ValueError):
            parse_intent(bad)

    def test_load_intent_from_file(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "intent.json"
            path.write_text(json.dumps(_intent_dict()))
            intent = load_intent(path)
            self.assertEqual(intent.scan.id, "bridge_test")


# ─── Plan ───────────────────────────────────────────────────────────


class TestParsePlan(unittest.TestCase):
    def test_minimal_plan(self) -> None:
        plan = parse_plan(_plan_dict())
        self.assertIsInstance(plan, Plan)
        self.assertEqual(plan.id, "plan_001")
        self.assertEqual(len(plan.sorties), 1)
        self.assertIsNotNone(plan.intent)

    def test_plan_without_embedded_intent(self) -> None:
        plan = parse_plan(_plan_dict(intent_embedded=False))
        self.assertIsNone(plan.intent)

    def test_sortie_steps_parsed(self) -> None:
        plan = parse_plan(_plan_dict(n_steps=3))
        sortie = plan.sorties[0]
        self.assertIsInstance(sortie, Sortie)
        self.assertEqual(sortie.drone_id, "drone-01")
        self.assertEqual(len(sortie.steps), 3)
        self.assertEqual(sortie.steps[0].index, 0)
        self.assertEqual(sortie.steps[2].index, 2)
        self.assertIsInstance(sortie.steps[0].waypoint, Waypoint)

    def test_schedule_total_duration_used(self) -> None:
        plan = parse_plan(_plan_dict())
        self.assertAlmostEqual(plan.total_duration_s, 60.0)

    def test_schedule_falls_back_to_summed_durations(self) -> None:
        data = _plan_dict(n_steps=4)
        del data["schedule"]
        del data["sorties"][0]["expected_duration_s"]
        plan = parse_plan(data)
        # 4 steps at 30 s each
        self.assertAlmostEqual(plan.total_duration_s, 120.0)

    def test_default_sortie_start_time_is_zero(self) -> None:
        plan = parse_plan(_plan_dict())
        self.assertAlmostEqual(plan.sorties[0].start_time_s, 0.0)

    def test_sequential_start_times_per_drone(self) -> None:
        data = _plan_dict()
        # Add a second sortie for the same drone, no explicit start_time_s
        second = json.loads(json.dumps(data["sorties"][0]))
        second["sortie_id"] = "sortie_002"
        data["sorties"].append(second)
        plan = parse_plan(data)
        self.assertEqual(len(plan.sorties), 2)
        self.assertAlmostEqual(plan.sorties[0].start_time_s, 0.0)
        self.assertAlmostEqual(plan.sorties[1].start_time_s, 60.0)

    def test_step_path_is_optional(self) -> None:
        plan = parse_plan(_plan_dict())
        self.assertIsNone(plan.sorties[0].steps[0].path)

    def test_step_with_explicit_path(self) -> None:
        data = _plan_dict()
        data["sorties"][0]["steps"][0]["path"] = [
            {"lat": 50.0, "lon": 14.0, "alt_m": 50.0},
            {"lat": 50.0001, "lon": 14.0, "alt_m": 50.0},
            {"lat": 50.0002, "lon": 14.0, "alt_m": 50.0},
        ]
        plan = parse_plan(data)
        path = plan.sorties[0].steps[0].path
        self.assertIsNotNone(path)
        assert path is not None  # for mypy
        self.assertEqual(len(path), 3)

    def test_load_plan_from_file(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "plan.json"
            path.write_text(json.dumps(_plan_dict()))
            plan = load_plan(path)
            self.assertEqual(plan.id, "plan_001")


if __name__ == "__main__":
    unittest.main()
