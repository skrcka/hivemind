"""Standalone CLI entry point for the plan-preview tool.

Run via Blender's headless mode:

    blender --background --python pantheon/hivemind_plan_preview/cli.py -- \\
        --plan plan.json --intent intent.json --out scene.blend

Blender consumes its own arguments before the literal `--`; this script
parses everything after `--` with argparse. The script also adds the parent
of this file to `sys.path` so it can `import hivemind_plan_preview` without
the package being installed as an extension.
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

# Make the package importable when run as a standalone script through
# `blender --background --python …`. We're inside the package directory, so
# the parent (pantheon/) is the path that needs to be on sys.path.
_PACKAGE_PARENT = Path(__file__).resolve().parent.parent
if str(_PACKAGE_PARENT) not in sys.path:
    sys.path.insert(0, str(_PACKAGE_PARENT))

from hivemind_plan_preview.coords import GpsOrigin  # noqa: E402


def main() -> int:
    args = _parse_args()

    # Defer bpy-touching imports until we actually need to build a scene.
    # This lets `python cli.py --help` work without Blender installed, which
    # is useful for argparse smoke tests.
    from hivemind_plan_preview.scene_builder import BuildOptions, build_scene

    options = BuildOptions(
        fps=args.fps,
        drone_marker_radius=args.marker_radius,
        sample_every_n_frames=args.sample_every_n,
        save_path=Path(args.out) if args.out else None,
        origin=_parse_origin(args.origin),
        clear_scene=not args.no_clear,
    )

    result = build_scene(
        plan_path=Path(args.plan),
        intent_path=Path(args.intent) if args.intent else None,
        options=options,
    )

    print(
        f"[hivemind_plan_preview] built {result.walls} walls + "
        f"{result.drones} drones, {result.keyframes} keyframes "
        f"over {result.total_duration_s:.1f}s"
    )
    if result.origin is not None:
        print(
            f"[hivemind_plan_preview] origin: "
            f"lat={result.origin.lat:.6f} lon={result.origin.lon:.6f} alt={result.origin.alt:.2f}"
        )
    for warning in result.warnings:
        print(f"[hivemind_plan_preview] WARN: {warning}")
    if options.save_path is not None:
        print(f"[hivemind_plan_preview] saved → {options.save_path}")
    return 0


def _parse_args() -> argparse.Namespace:
    # Blender swallows its own args before "--"; everything after is ours.
    # When run via stock python (e.g. `python cli.py --help`) there's no "--",
    # so we just skip argv[0].
    argv = sys.argv
    argv = argv[argv.index("--") + 1 :] if "--" in argv else argv[1:]

    parser = argparse.ArgumentParser(
        prog="hivemind_plan_preview",
        description="Convert a Hivemind plan + intent into a Blender scene with animated drones.",
    )
    parser.add_argument(
        "--plan",
        required=True,
        help="Path to the HivemindPlan JSON file (oracle slicer output)",
    )
    parser.add_argument(
        "--intent",
        default=None,
        help="Path to intent JSON (optional if the plan embeds it)",
    )
    parser.add_argument(
        "--out",
        default=None,
        help="Output .blend file. If omitted, the scene is built in the current Blender doc only.",
    )
    parser.add_argument("--fps", type=int, default=24)
    parser.add_argument("--marker-radius", type=float, default=0.5)
    parser.add_argument(
        "--sample-every-n",
        type=int,
        default=1,
        help="Lower = denser keyframes; higher = lighter scene",
    )
    parser.add_argument(
        "--origin",
        default=None,
        help='GPS origin "lat,lon,alt" — defaults to first waypoint of first sortie',
    )
    parser.add_argument(
        "--no-clear",
        action="store_true",
        help="Don't clear the existing scene before building",
    )
    return parser.parse_args(argv)


def _parse_origin(value: str | None) -> GpsOrigin | None:
    if value is None:
        return None
    parts = value.split(",")
    if len(parts) != 3:
        raise SystemExit(f"--origin must be 'lat,lon,alt' — got {value!r}")
    try:
        return GpsOrigin(lat=float(parts[0]), lon=float(parts[1]), alt=float(parts[2]))
    except ValueError as exc:
        raise SystemExit(f"--origin parse error: {exc}") from exc


if __name__ == "__main__":
    sys.exit(main())
