import type { ArmingState, TelemetrySnapshot } from "../lib/tauri";

interface Props {
  telemetry: TelemetrySnapshot;
  arming: ArmingState;
}

/**
 * A read-only indicator for pump state and interlock readiness. Praetor
 * fires the pump from the controller's A button; this component exists so
 * the operator can see *why* it's refusing to fire (too low, disarmed,
 * stale link). It deliberately has no button — pump control is a
 * physical action, not a UI click.
 */
export function PumpIndicator({ telemetry, arming }: Props) {
  const armed = arming.kind === "armed";
  const alt = telemetry.position.relative_alt_m;
  const aboveMin = alt >= 0.5;
  // We can't observe the pump's actual state over MAVLink (there's no
  // feedback message from a DO_SET_SERVO), so the "on" indicator is the
  // interlock-green state — the operator infers actual spray from the
  // hold-A press they just made.
  const ready = armed && aboveMin;

  const reason =
    !armed
      ? "disarmed"
      : !aboveMin
        ? `altitude ${alt.toFixed(1)} m < 0.5 m`
        : "ready";

  return (
    <div className="panel">
      <h3>Pump</h3>
      <div className="pump-indicator">
        <div className={`pump-led ${ready ? "on" : ""}`} />
        <span className="pump-state">{ready ? "READY" : "refused"}</span>
      </div>
      <p className="pump-reason">{reason}</p>
      <p className="pump-hint">Hold A on the controller to spray.</p>
      <style>{`
        .pump-state {
          font-weight: 700;
          font-family: "SF Mono", monospace;
          color: var(--fg);
        }
        .pump-reason {
          margin: 8px 0 0;
          font-size: 11px;
          color: var(--fg-muted);
          font-family: "SF Mono", monospace;
        }
        .pump-hint {
          margin: 4px 0 0;
          font-size: 11px;
          color: var(--fg-muted);
        }
      `}</style>
    </div>
  );
}
