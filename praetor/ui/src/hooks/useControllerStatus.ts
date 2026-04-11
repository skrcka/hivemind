import { useEffect, useState } from "react";
import { listenController, type ControllerStatus } from "../lib/tauri";

export function useControllerStatus(): ControllerStatus {
  const [status, setStatus] = useState<ControllerStatus>("disconnected");

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    listenController(setStatus).then((fn) => {
      unlisten = fn;
    });
    return () => {
      unlisten?.();
    };
  }, []);

  return status;
}
