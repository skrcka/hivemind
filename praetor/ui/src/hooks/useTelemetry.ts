import { useEffect, useState } from "react";
import {
  listenTelemetry,
  type TelemetrySnapshot,
} from "../lib/tauri";

const INITIAL: TelemetrySnapshot = {
  attitude: { roll_rad: 0, pitch_rad: 0, yaw_rad: 0 },
  position: {
    lat_deg: 0,
    lon_deg: 0,
    alt_msl_m: 0,
    relative_alt_m: 0,
    vx_m_s: 0,
    vy_m_s: 0,
    vz_m_s: 0,
  },
  battery: { voltage_v: 0, current_a: 0, remaining_pct: -1 },
  gps: { fix: "none", satellites_visible: 0, eph_cm: 0, epv_cm: 0 },
  tof_distance_m: null,
  radio: { rssi: 0, remrssi: 0, noise: 0, remnoise: 0 },
  heartbeat: {
    armed: false,
    custom_mode: 0,
    base_mode: 0,
    system_status: 0,
    age_ms: 0,
  },
  updated_at_ms: 0,
};

export function useTelemetry(): TelemetrySnapshot {
  const [snap, setSnap] = useState<TelemetrySnapshot>(INITIAL);

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    listenTelemetry(setSnap).then((fn) => {
      unlisten = fn;
    });
    return () => {
      unlisten?.();
    };
  }, []);

  return snap;
}
