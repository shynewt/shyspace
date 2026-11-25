import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect, useState } from "react";
import { reportError } from "./toast";

/** Whether the window fills the screen; Tauri reports entering and leaving full screen as a resize. */
export function useIsFullscreen(): boolean {
  const [isFullscreen, setIsFullscreen] = useState(false);
  useEffect(() => {
    let isCurrent = true;
    const appWindow = getCurrentWindow();
    const check = () => appWindow.isFullscreen().then((value) => isCurrent && setIsFullscreen(value), reportError);
    check();
    const stopListening = appWindow.onResized(check);
    return () => {
      isCurrent = false;
      stopListening.then((stop) => stop(), reportError);
    };
  }, []);
  return isFullscreen;
}
