import { useEffect, useState } from "react";
import { listenArming, type ArmingState } from "../lib/tauri";

const INITIAL: ArmingState = { kind: "disarmed", progress: 0 };

export function useArming(): ArmingState {
  const [state, setState] = useState<ArmingState>(INITIAL);

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    listenArming(setState).then((fn) => {
      unlisten = fn;
    });
    return () => {
      unlisten?.();
    };
  }, []);

  return state;
}
