import { useCallback } from "react";
import {
  beginArming,
  cancelArming,
  type ArmingState,
  type LinkStatus,
} from "../lib/tauri";

interface Props {
  arming: ArmingState;
  linkStatus: LinkStatus;
}

export function ArmButton({ arming, linkStatus }: Props) {
  const disabled = linkStatus !== "connected";
  const armed = arming.kind === "armed";
  const progress = arming.progress;

  const onPointerDown = useCallback(() => {
    if (disabled || armed) return;
    beginArming().catch(() => {});
  }, [disabled, armed]);

  const onPointerUp = useCallback(() => {
    if (disabled) return;
    cancelArming().catch(() => {});
  }, [disabled]);

  const label = armed
    ? "ARMED — release to stay armed"
    : arming.kind === "arming"
      ? `Arming… hold (${Math.round(progress * 100)}%)`
      : "Hold to ARM (3 s)";

  return (
    <div className="panel">
      <h3>Arming</h3>
      <button
        className={`arm-button ${armed ? "btn-danger" : "btn-primary"}`}
        disabled={disabled}
        onPointerDown={onPointerDown}
        onPointerUp={onPointerUp}
        onPointerLeave={onPointerUp}
        onPointerCancel={onPointerUp}
        type="button"
      >
        {label}
        <div
          className="arm-progress"
          style={{ width: `${progress * 100}%` }}
        />
      </button>
      <p className="arm-hint">
        Alternative: hold LB + RB on the controller.
      </p>
      <style>{`
        .arm-hint { color: var(--fg-muted); font-size: 11px; margin: 6px 0 0; text-align: center; }
      `}</style>
    </div>
  );
}
