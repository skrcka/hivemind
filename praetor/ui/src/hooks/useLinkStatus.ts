import { useEffect, useState } from "react";
import { listenLinkStatus, type LinkStatus } from "../lib/tauri";

export function useLinkStatus(): LinkStatus {
  const [status, setStatus] = useState<LinkStatus>("disconnected");

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    listenLinkStatus(setStatus).then((fn) => {
      unlisten = fn;
    });
    return () => {
      unlisten?.();
    };
  }, []);

  return status;
}
