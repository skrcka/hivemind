import { useCallback, useEffect, useRef, useState } from "react";
import { emergencyStop, type LinkStatus } from "../lib/tauri";

const HOLD_MS = 1000;

interface Props {
  linkStatus: LinkStatus;
}

export function EmergencyStop({ linkStatus }: Props) {
  const [progress, setProgress] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const startRef = useRef<number | null>(null);
  const rafRef = useRef<number | null>(null);
  const firedRef = useRef(false);

  const disabled = linkStatus !== "connected" && linkStatus !== "stale";

  const stop = useCallback(() => {
    startRef.current = null;
    firedRef.current = false;
    if (rafRef.current !== null) {
      cancelAnimationFrame(rafRef.current);
      rafRef.current = null;
    }
    setProgress(0);
  }, []);

  const tick = useCallback(() => {
    if (startRef.current === null) return;
    const elapsed = performance.now() - startRef.current;
    const p = Math.min(1, elapsed / HOLD_MS);
    setProgress(p);
    if (p >= 1 && !firedRef.current) {
      firedRef.current = true;
      emergencyStop()
        .then(() => setError(null))
        .catch((e) => setError(String(e)));
      stop();
      return;
    }
    rafRef.current = requestAnimationFrame(tick);
  }, [stop]);

  const start = useCallback(() => {
    if (disabled) return;
    startRef.current = performance.now();
    rafRef.current = requestAnimationFrame(tick);
  }, [disabled, tick]);

  useEffect(() => () => stop(), [stop]);

  return (
    <div className="panel">
      <h3>Emergency</h3>
      <button
        className="btn-danger estop-button"
        disabled={disabled}
        onPointerDown={start}
        onPointerUp={stop}
        onPointerLeave={stop}
        onPointerCancel={stop}
        type="button"
      >
        HOLD 1 s — MOTOR CUT
        <div
          className="arm-progress"
          style={{ width: `${progress * 100}%`, background: "white" }}
        />
      </button>
      {error && <div className="estop-error">{error}</div>}
      <style>{`
        .estop-button {
          position: relative;
          width: 100%;
          padding: 24px 12px;
          font-size: 14px;
        }
        .estop-error {
          margin-top: 8px;
          padding: 6px 8px;
          background: rgba(239, 68, 68, 0.15);
          color: var(--bad);
          border: 1px solid var(--bad);
          border-radius: 4px;
          font-size: 11px;
        }
      `}</style>
    </div>
  );
}
