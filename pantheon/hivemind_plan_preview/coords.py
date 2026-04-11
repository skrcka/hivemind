"""GPS ↔ local ENU coordinate conversion.

Uses a flat-earth approximation that is accurate to a few centimetres for
small areas (a bridge spans at most a few hundred metres). Suitable for v1
visualization only — never use this for actual navigation.

Has NO Blender dependency.
"""

from __future__ import annotations

import math
from dataclasses import dataclass

# Length of one degree of latitude at the equator. Latitude scaling is roughly
# constant; longitude scaling shrinks with cos(latitude).
EARTH_METRES_PER_DEGREE = 111_320.0


@dataclass(frozen=True)
class GpsOrigin:
    """The reference point that the local ENU frame is centred on."""

    lat: float
    lon: float
    alt: float


@dataclass(frozen=True)
class EnuPoint:
    """A point in metres in the local East-North-Up frame around a GpsOrigin."""

    east: float
    north: float
    up: float


def gps_to_enu(lat: float, lon: float, alt: float, origin: GpsOrigin) -> EnuPoint:
    """Project a GPS point into local ENU metres around `origin`.

    East is the +X axis, north is +Y, up is +Z (matches Blender's right-handed
    convention when the scene's units are metric).
    """
    d_lat = lat - origin.lat
    d_lon = lon - origin.lon
    cos_origin_lat = math.cos(math.radians(origin.lat))
    return EnuPoint(
        east=d_lon * EARTH_METRES_PER_DEGREE * cos_origin_lat,
        north=d_lat * EARTH_METRES_PER_DEGREE,
        up=alt - origin.alt,
    )


def enu_to_gps(
    east: float, north: float, up: float, origin: GpsOrigin
) -> tuple[float, float, float]:
    """Inverse of `gps_to_enu`. Used by tests for round-trip checks."""
    cos_origin_lat = math.cos(math.radians(origin.lat))
    return (
        origin.lat + north / EARTH_METRES_PER_DEGREE,
        origin.lon + east / (EARTH_METRES_PER_DEGREE * cos_origin_lat),
        origin.alt + up,
    )
