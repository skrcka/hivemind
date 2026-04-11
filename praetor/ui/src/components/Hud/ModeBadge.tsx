import type { TelemetrySnapshot } from "../../lib/tauri";

interface Props {
  telemetry: TelemetrySnapshot;
}

// PX4 custom_mode values for the modes the operator cares about.
// See PX4 commander_state_machine.cpp for the authoritative list.
// These are the main-mode / sub-mode pairs packed into the u32.
//
// Main mode shift = 16, sub mode shift = 24.
const PX4_CUSTOM_MODES: Record<number, string> = {
  // main_mode = 1 — MANUAL
  0x01010000: "MANUAL",
  // main_mode = 2 — ALTCTL
  0x02010000: "ALT HOLD",
  // main_mode = 3 — POSCTL
  0x03010000: "POSITION",
  // main_mode = 4 — AUTO
  0x04030000: "AUTO TAKEOFF",
  0x04040000: "AUTO HOLD",
  0x04050000: "AUTO MISSION",
  0x04060000: "AUTO RTL",
  0x04070000: "AUTO LAND",
  // main_mode = 5 — ACRO
  0x05010000: "ACRO",
  // main_mode = 6 — OFFBOARD
  0x06010000: "OFFBOARD",
  // main_mode = 7 — STAB
  0x07010000: "STABILIZED",
};

function decodePx4Mode(customMode: number): string {
  if (customMode === 0) return "—";
  const known = PX4_CUSTOM_MODES[customMode];
  if (known) return known;
  // Fall back to showing just the main-mode byte in hex.
  const main = (customMode >> 16) & 0xff;
  return `MODE 0x${main.toString(16).padStart(2, "0")}`;
}

export function ModeBadge({ telemetry }: Props) {
  const mode = decodePx4Mode(telemetry.heartbeat.custom_mode);
  const armed = telemetry.heartbeat.armed;

  return (
    <span
      className="badge mode-badge"
      style={{
        color: armed ? "var(--bad)" : "var(--fg-muted)",
        borderColor: armed ? "var(--bad)" : "var(--border)",
      }}
    >
      {armed ? "◉ ARMED" : "○ safe"} · {mode}
    </span>
  );
}
