"""Unit tests for hivemind_plan_preview.coords.

Pure-Python tests — no Blender required. The package's __init__.py defers
its bpy imports into register(), so importing the pure submodules directly
does not pull in any bpy dependency.
"""

from __future__ import annotations

import math
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from hivemind_plan_preview.coords import (
    EARTH_METRES_PER_DEGREE,
    GpsOrigin,
    enu_to_gps,
    gps_to_enu,
)


def _origin() -> GpsOrigin:
    # Charles Bridge, Prague — close enough for a 100 m bridge
    return GpsOrigin(lat=50.0865, lon=14.4114, alt=200.0)


class TestGpsToEnu(unittest.TestCase):
    def test_origin_maps_to_zero(self) -> None:
        origin = _origin()
        enu = gps_to_enu(origin.lat, origin.lon, origin.alt, origin)
        self.assertAlmostEqual(enu.east, 0.0, places=6)
        self.assertAlmostEqual(enu.north, 0.0, places=6)
        self.assertAlmostEqual(enu.up, 0.0, places=6)

    def test_north_positive_lat(self) -> None:
        origin = _origin()
        # 1 metre north
        delta_lat = 1.0 / EARTH_METRES_PER_DEGREE
        enu = gps_to_enu(origin.lat + delta_lat, origin.lon, origin.alt, origin)
        self.assertAlmostEqual(enu.east, 0.0, places=6)
        self.assertAlmostEqual(enu.north, 1.0, places=6)

    def test_east_positive_lon(self) -> None:
        origin = _origin()
        cos_lat = math.cos(math.radians(origin.lat))
        # 1 metre east
        delta_lon = 1.0 / (EARTH_METRES_PER_DEGREE * cos_lat)
        enu = gps_to_enu(origin.lat, origin.lon + delta_lon, origin.alt, origin)
        self.assertAlmostEqual(enu.east, 1.0, places=6)
        self.assertAlmostEqual(enu.north, 0.0, places=6)

    def test_altitude_passthrough(self) -> None:
        origin = _origin()
        enu = gps_to_enu(origin.lat, origin.lon, origin.alt + 17.5, origin)
        self.assertAlmostEqual(enu.up, 17.5, places=6)

    def test_southwest_negative(self) -> None:
        origin = _origin()
        delta = 1.0 / EARTH_METRES_PER_DEGREE
        enu = gps_to_enu(origin.lat - delta, origin.lon - delta, origin.alt, origin)
        self.assertLess(enu.north, 0.0)
        self.assertLess(enu.east, 0.0)


class TestRoundTrip(unittest.TestCase):
    def test_round_trip_at_origin(self) -> None:
        origin = _origin()
        lat, lon, alt = enu_to_gps(0.0, 0.0, 0.0, origin)
        self.assertAlmostEqual(lat, origin.lat, places=10)
        self.assertAlmostEqual(lon, origin.lon, places=10)
        self.assertAlmostEqual(alt, origin.alt, places=10)

    def test_round_trip_arbitrary_point(self) -> None:
        origin = _origin()
        # 50 metres east, 25 north, 10 up
        enu_in = (50.0, 25.0, 10.0)
        lat, lon, alt = enu_to_gps(*enu_in, origin)
        out = gps_to_enu(lat, lon, alt, origin)
        self.assertAlmostEqual(out.east, enu_in[0], places=4)
        self.assertAlmostEqual(out.north, enu_in[1], places=4)
        self.assertAlmostEqual(out.up, enu_in[2], places=4)


if __name__ == "__main__":
    unittest.main()
