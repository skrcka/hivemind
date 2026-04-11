import { useEffect, useState } from "react";
import {
  connect,
  disconnect,
  listSerialPorts,
  type LinkStatus,
} from "../lib/tauri";

interface Props {
  linkStatus: LinkStatus;
}

export function ConnectionPanel({ linkStatus }: Props) {
  const [address, setAddress] = useState<string>("tcp:127.0.0.1:5760");
  const [ports, setPorts] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    listSerialPorts().then(setPorts).catch(() => {
      /* ignore; just show an empty list */
    });
  }, []);

  const connected = linkStatus === "connected" || linkStatus === "stale";

  const onConnect = async () => {
    setError(null);
    try {
      await connect(address);
    } catch (e) {
      setError(String(e));
    }
  };

  const onDisconnect = async () => {
    setError(null);
    try {
      await disconnect();
    } catch (e) {
      setError(String(e));
    }
  };

  return (
    <div className="panel">
      <h3>Connection</h3>
      <div className="connection-row">
        <input
          type="text"
          value={address}
          onChange={(e) => setAddress(e.target.value)}
          disabled={connected}
          placeholder="serial:/dev/ttyUSB1:57600 or tcp:127.0.0.1:5760"
        />
      </div>

      {ports.length > 0 && !connected && (
        <div className="ports">
          <span className="ports-label">serial ports:</span>
          {ports.map((p) => (
            <button
              key={p}
              className="port-button"
              onClick={() => setAddress(`serial:${p}:57600`)}
              type="button"
            >
              {p}
            </button>
          ))}
        </div>
      )}

      <div className="connection-buttons">
        {!connected ? (
          <button className="btn-primary" onClick={onConnect} type="button">
            Connect
          </button>
        ) : (
          <button onClick={onDisconnect} type="button">
            Disconnect
          </button>
        )}
      </div>

      {error && <div className="error">{error}</div>}

      <style>{`
        .connection-row input {
          width: 100%;
          padding: 8px;
          background: var(--bg-panel-hi);
          border: 1px solid var(--border);
          border-radius: 4px;
          color: var(--fg);
          font-family: "SF Mono", monospace;
          font-size: 12px;
        }
        .ports { display: flex; flex-wrap: wrap; gap: 4px; margin: 8px 0; }
        .ports-label { color: var(--fg-muted); font-size: 11px; width: 100%; }
        .port-button { font-size: 11px; padding: 4px 8px; font-family: "SF Mono", monospace; }
        .connection-buttons { margin-top: 10px; }
        .connection-buttons button { width: 100%; }
        .error {
          margin-top: 8px;
          padding: 6px 8px;
          background: rgba(239, 68, 68, 0.1);
          color: var(--bad);
          border: 1px solid var(--bad);
          border-radius: 4px;
          font-size: 12px;
        }
      `}</style>
    </div>
  );
}
