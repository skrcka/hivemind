import { AttitudeIndicator } from "./components/Hud/AttitudeIndicator";
import { BatteryGauge } from "./components/Hud/BatteryGauge";
import { GpsBadge } from "./components/Hud/GpsBadge";
import { ModeBadge } from "./components/Hud/ModeBadge";
import { ConnectionPanel } from "./components/ConnectionPanel";
import { ArmButton } from "./components/ArmButton";
import { EmergencyStop } from "./components/EmergencyStop";
import { PumpIndicator } from "./components/PumpIndicator";
import { useTelemetry } from "./hooks/useTelemetry";
import { useLinkStatus } from "./hooks/useLinkStatus";
import { useControllerStatus } from "./hooks/useControllerStatus";
import { useArming } from "./hooks/useArming";

export default function App() {
  const telemetry = useTelemetry();
  const linkStatus = useLinkStatus();
  const controller = useControllerStatus();
  const arming = useArming();

  return (
    <div className="app">
      <header className="topbar">
        <h1>Praetor</h1>
        <span className="subtitle">Hivemind manual control</span>
        <div className="status-badges">
          <span className={`badge badge-link badge-${linkStatus}`}>
            link: {linkStatus}
          </span>
          <span className={`badge badge-ctrl badge-${controller}`}>
            pad: {controller}
          </span>
          <ModeBadge telemetry={telemetry} />
        </div>
      </header>

      <main className="hud">
        <div className="hud-col hud-col-left">
          <ConnectionPanel linkStatus={linkStatus} />
          <BatteryGauge battery={telemetry.battery} />
          <GpsBadge gps={telemetry.gps} />
          <PumpIndicator telemetry={telemetry} arming={arming} />
        </div>

        <div className="hud-col hud-col-center">
          <AttitudeIndicator attitude={telemetry.attitude} />
          <div className="readout">
            <div className="readout-item">
              <span className="label">ALT</span>
              <span className="value">
                {telemetry.position.relative_alt_m.toFixed(1)} m
              </span>
            </div>
            <div className="readout-item">
              <span className="label">HDG</span>
              <span className="value">
                {((telemetry.attitude.yaw_rad * 180) / Math.PI).toFixed(0)}°
              </span>
            </div>
            <div className="readout-item">
              <span className="label">ToF</span>
              <span className="value">
                {telemetry.tof_distance_m != null
                  ? `${telemetry.tof_distance_m.toFixed(2)} m`
                  : "—"}
              </span>
            </div>
          </div>
        </div>

        <div className="hud-col hud-col-right">
          <ArmButton arming={arming} linkStatus={linkStatus} />
          <EmergencyStop linkStatus={linkStatus} />
        </div>
      </main>
    </div>
  );
}
