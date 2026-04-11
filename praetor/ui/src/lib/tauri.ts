/**
 * Typed wrappers around `@tauri-apps/api` — keeps the rest of the frontend
 * from touching the string-typed `invoke` / `listen` APIs directly.
 */

import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { listen as tauriListen, type UnlistenFn } from "@tauri-apps/api/event";

// ── Shared types (mirrors src/mavlink_link/snapshot.rs and state.rs) ──

export type LinkStatus =
  | "disconnected"
  | "connecting"
  | "connected"
  | "stale"
  | "failed";

export type ArmingKind = "disarmed" | "arming" | "armed" | "disarming";

export interface ArmingState {
  kind: ArmingKind;
  progress: number; // 0..1
}

export type ControllerStatus = "disconnected" | "connected";

export type GpsFix =
  | "none"
  | "fix2d"
  | "fix3d"
  | "dgps"
  | "rtk_float"
  | "rtk_fixed";

export interface Attitude {
  roll_rad: number;
  pitch_rad: number;
  yaw_rad: number;
}

export interface Position {
  lat_deg: number;
  lon_deg: number;
  alt_msl_m: number;
  relative_alt_m: number;
  vx_m_s: number;
  vy_m_s: number;
  vz_m_s: number;
}

export interface Battery {
  voltage_v: number;
  current_a: number;
  remaining_pct: number;
}

export interface Gps {
  fix: GpsFix;
  satellites_visible: number;
  eph_cm: number;
  epv_cm: number;
}

export interface Radio {
  rssi: number;
  remrssi: number;
  noise: number;
  remnoise: number;
}

export interface Heartbeat {
  armed: boolean;
  custom_mode: number;
  base_mode: number;
  system_status: number;
  age_ms: number;
}

export interface TelemetrySnapshot {
  attitude: Attitude;
  position: Position;
  battery: Battery;
  gps: Gps;
  tof_distance_m: number | null;
  radio: Radio;
  heartbeat: Heartbeat;
  updated_at_ms: number;
}

// ── Commands ─────────────────────────────────────────────────────────

export async function connect(address?: string): Promise<void> {
  await tauriInvoke("connect", { address });
}

export async function disconnect(): Promise<void> {
  await tauriInvoke("disconnect");
}

export async function beginArming(): Promise<void> {
  await tauriInvoke("begin_arming");
}

export async function cancelArming(): Promise<void> {
  await tauriInvoke("cancel_arming");
}

export async function emergencyStop(): Promise<void> {
  await tauriInvoke("emergency_stop");
}

export async function takeoff(): Promise<void> {
  await tauriInvoke("takeoff");
}

export async function land(): Promise<void> {
  await tauriInvoke("land");
}

export async function returnToLaunch(): Promise<void> {
  await tauriInvoke("return_to_launch");
}

export async function cycleMode(): Promise<void> {
  await tauriInvoke("cycle_mode");
}

export async function listSerialPorts(): Promise<string[]> {
  return await tauriInvoke<string[]>("list_serial_ports");
}

// ── Events ───────────────────────────────────────────────────────────

export function listenTelemetry(
  cb: (snap: TelemetrySnapshot) => void,
): Promise<UnlistenFn> {
  return tauriListen<TelemetrySnapshot>("telemetry_update", (evt) =>
    cb(evt.payload),
  );
}

export function listenLinkStatus(
  cb: (status: LinkStatus) => void,
): Promise<UnlistenFn> {
  return tauriListen<LinkStatus>("link_status", (evt) => cb(evt.payload));
}

export function listenArming(
  cb: (state: ArmingState) => void,
): Promise<UnlistenFn> {
  return tauriListen<ArmingState>("arming_state", (evt) => cb(evt.payload));
}

export function listenController(
  cb: (status: ControllerStatus) => void,
): Promise<UnlistenFn> {
  return tauriListen<ControllerStatus>("controller_status", (evt) =>
    cb(evt.payload),
  );
}
