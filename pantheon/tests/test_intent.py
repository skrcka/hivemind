"""Unit tests for hivemind_plane_picker.intent.

These tests run with stock Python (no bpy required) because intent.py is a
pure data module. Run them via:

    make test

The test imports intent.py directly (bypassing the package __init__.py) so
that the Blender-only sibling modules (operators.py, ui.py, properties.py)
don't get loaded when bpy is unavailable.
"""

from __future__ import annotations

import importlib.util
import json
import sys
import tempfile
import unittest
from pathlib import Path

_INTENT_PATH = Path(__file__).resolve().parent.parent / "hivemind_plane_picker" / "intent.py"
_spec = importlib.util.spec_from_file_location("hivemind_intent", _INTENT_PATH)
assert _spec is not None and _spec.loader is not None
intent = importlib.util.module_from_spec(_spec)
# Register in sys.modules BEFORE exec_module so dataclass introspection works
# under `from __future__ import annotations` (Python 3.13+).
sys.modules["hivemind_intent"] = intent
_spec.loader.exec_module(intent)

INTENT_SCHEMA_VERSION = intent.INTENT_SCHEMA_VERSION
Face = intent.Face
Region = intent.Region
Scan = intent.Scan
build_intent = intent.build_intent
write_intent = intent.write_intent


def _make_face() -> Face:
    return Face(
        vertices=((0.0, 0.0, 0.0), (1.0, 0.0, 0.0), (0.0, 1.0, 0.0)),
        normal=(0.0, 0.0, 1.0),
    )


def _make_region(name: str = "north_face", n_faces: int = 1) -> Region:
    return Region(
        id=name,
        name=name,
        faces=tuple(_make_face() for _ in range(n_faces)),
        area_m2=0.5 * n_faces,
    )


def _make_scan(georef: bool = True) -> Scan:
    return Scan(id="bridge_2026", source_file="/tmp/bridge.obj", georeferenced=georef)


class TestBuildIntent(unittest.TestCase):
    def test_version_field_matches_constant(self) -> None:
        doc = build_intent(_make_scan(), ())
        self.assertEqual(doc["version"], INTENT_SCHEMA_VERSION)
        self.assertEqual(doc["version"], "1.0")

    def test_empty_regions(self) -> None:
        doc = build_intent(_make_scan(), ())
        self.assertEqual(doc["regions"], [])
        self.assertEqual(doc["constraints"], {})

    def test_scan_passthrough(self) -> None:
        doc = build_intent(_make_scan(georef=False), ())
        self.assertEqual(doc["scan"]["id"], "bridge_2026")
        self.assertEqual(doc["scan"]["source_file"], "/tmp/bridge.obj")
        self.assertFalse(doc["scan"]["georeferenced"])

    def test_single_region(self) -> None:
        doc = build_intent(_make_scan(), (_make_region("north", 1),))
        self.assertEqual(len(doc["regions"]), 1)
        region = doc["regions"][0]
        self.assertEqual(region["id"], "north")
        self.assertEqual(region["name"], "north")
        self.assertEqual(len(region["faces"]), 1)
        self.assertAlmostEqual(region["area_m2"], 0.5)

    def test_face_geometry_serialized_as_lists(self) -> None:
        doc = build_intent(_make_scan(), (_make_region("r", 1),))
        face = doc["regions"][0]["faces"][0]
        # vertices and normal must be JSON-friendly lists, not tuples
        self.assertIsInstance(face["vertices"], list)
        self.assertEqual(len(face["vertices"]), 3)
        for v in face["vertices"]:
            self.assertIsInstance(v, list)
            self.assertEqual(len(v), 3)
        self.assertIsInstance(face["normal"], list)
        self.assertEqual(len(face["normal"]), 3)

    def test_multiple_regions_preserved_in_order(self) -> None:
        regions = (
            _make_region("a", 2),
            _make_region("b", 5),
            _make_region("c", 1),
        )
        doc = build_intent(_make_scan(), regions)
        self.assertEqual(len(doc["regions"]), 3)
        self.assertEqual([r["id"] for r in doc["regions"]], ["a", "b", "c"])
        self.assertEqual([len(r["faces"]) for r in doc["regions"]], [2, 5, 1])

    def test_constraints_passthrough(self) -> None:
        doc = build_intent(_make_scan(), (), {"max_drones": 6})
        self.assertEqual(doc["constraints"], {"max_drones": 6})

    def test_constraints_default_empty(self) -> None:
        doc = build_intent(_make_scan(), ())
        self.assertEqual(doc["constraints"], {})

    def test_constraints_dict_is_copied_not_referenced(self) -> None:
        original = {"max_drones": 6}
        doc = build_intent(_make_scan(), (), original)
        original["max_drones"] = 999
        self.assertEqual(doc["constraints"]["max_drones"], 6)

    def test_output_is_json_serializable(self) -> None:
        doc = build_intent(_make_scan(), (_make_region("r", 3),), {"k": "v"})
        # Should not raise
        json.dumps(doc)


class TestWriteIntent(unittest.TestCase):
    def test_writes_valid_json_file(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            path = Path(tmpdir) / "out.json"
            write_intent(path, _make_scan(), (_make_region("r", 2),))

            self.assertTrue(path.exists())
            data = json.loads(path.read_text(encoding="utf-8"))
            self.assertEqual(data["version"], "1.0")
            self.assertEqual(len(data["regions"]), 1)
            self.assertEqual(len(data["regions"][0]["faces"]), 2)

    def test_round_trip_preserves_data(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            path = Path(tmpdir) / "out.json"
            scan = _make_scan(georef=True)
            regions = (_make_region("alpha", 1), _make_region("beta", 3))
            write_intent(path, scan, regions, {"weather": "sunny"})

            data = json.loads(path.read_text(encoding="utf-8"))
            self.assertEqual(data["scan"]["id"], "bridge_2026")
            self.assertTrue(data["scan"]["georeferenced"])
            self.assertEqual([r["name"] for r in data["regions"]], ["alpha", "beta"])
            self.assertEqual(data["constraints"]["weather"], "sunny")

    def test_accepts_string_path(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            path = str(Path(tmpdir) / "out.json")
            write_intent(path, _make_scan(), ())
            self.assertTrue(Path(path).exists())

    def test_writes_utf8_encoded(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            path = Path(tmpdir) / "out.json"
            scan = Scan(id="bröcke_ñ", source_file="", georeferenced=False)
            write_intent(path, scan, (_make_region("región", 1),))

            data = json.loads(path.read_text(encoding="utf-8"))
            self.assertEqual(data["scan"]["id"], "bröcke_ñ")
            self.assertEqual(data["regions"][0]["id"], "región")


if __name__ == "__main__":
    unittest.main()
