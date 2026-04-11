import type { Gps } from "../../lib/tauri";

interface Props {
  gps: Gps;
}

const FIX_LABEL: Record<string, string> = {
  none: "NO FIX",
  fix2d: "2D",
  fix3d: "3D",
  dgps: "DGPS",
  rtk_float: "RTK FLT",
  rtk_fixed: "RTK FIX",
};

export function GpsBadge({ gps }: Props) {
  const fixColor =
    gps.fix === "rtk_fixed"
      ? "var(--ok)"
      : gps.fix === "rtk_float" || gps.fix === "fix3d"
        ? "var(--warn)"
        : "var(--bad)";

  return (
    <div className="panel">
      <h3>GPS</h3>
      <div className="gps-row">
        <span className="gps-fix" style={{ color: fixColor }}>
          {FIX_LABEL[gps.fix] ?? gps.fix}
        </span>
        <span className="gps-sats">{gps.satellites_visible} sats</span>
      </div>
      <div className="gps-row gps-row-muted">
        <span>HDOP {(gps.eph_cm / 100).toFixed(1)} m</span>
        <span>VDOP {(gps.epv_cm / 100).toFixed(1)} m</span>
      </div>
      <style>{`
        .gps-row { display: flex; justify-content: space-between; align-items: center; margin: 4px 0; }
        .gps-fix { font-size: 18px; font-weight: 700; font-family: "SF Mono", monospace; }
        .gps-sats { font-family: "SF Mono", monospace; color: var(--fg-muted); }
        .gps-row-muted { font-size: 11px; color: var(--fg-muted); font-family: "SF Mono", monospace; }
      `}</style>
    </div>
  );
}
