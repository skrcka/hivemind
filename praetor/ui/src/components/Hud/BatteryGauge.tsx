import type { Battery } from "../../lib/tauri";

interface Props {
  battery: Battery;
}

export function BatteryGauge({ battery }: Props) {
  const pct = battery.remaining_pct;
  const displayPct = pct < 0 ? "—" : `${pct}%`;
  const barWidth = pct < 0 ? 0 : Math.max(0, Math.min(100, pct));

  const colorClass =
    pct < 0
      ? "battery-unknown"
      : pct < 20
        ? "battery-critical"
        : pct < 40
          ? "battery-low"
          : "battery-ok";

  return (
    <div className="panel">
      <h3>Battery</h3>
      <div className="battery-readout">
        <div className="battery-pct">{displayPct}</div>
        <div className="battery-bar-outer">
          <div
            className={`battery-bar-inner ${colorClass}`}
            style={{ width: `${barWidth}%` }}
          />
        </div>
        <div className="battery-stats">
          <span>{battery.voltage_v.toFixed(1)} V</span>
          <span>{battery.current_a.toFixed(1)} A</span>
        </div>
      </div>
      <style>{`
        .battery-readout { display: flex; flex-direction: column; gap: 6px; }
        .battery-pct { font-size: 32px; font-weight: 700; font-family: "SF Mono", monospace; }
        .battery-bar-outer { height: 8px; background: var(--bg-panel-hi); border-radius: 4px; overflow: hidden; }
        .battery-bar-inner { height: 100%; transition: width 200ms; }
        .battery-ok { background: var(--ok); }
        .battery-low { background: var(--warn); }
        .battery-critical { background: var(--bad); }
        .battery-unknown { background: var(--fg-muted); }
        .battery-stats { display: flex; justify-content: space-between; color: var(--fg-muted); font-family: "SF Mono", monospace; font-size: 12px; }
      `}</style>
    </div>
  );
}
