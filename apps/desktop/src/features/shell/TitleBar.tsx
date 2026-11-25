import { isMac } from "../../lib/platform";
import { useIsFullscreen } from "../../lib/useIsFullscreen";

/**
 * macOS draws the window's buttons over the top of the page (the window uses
 * an overlay title bar), so this strip makes room for them in the app's own
 * colours and moves the window when dragged. Full screen hides the buttons,
 * so the strip goes too. Windows and Linux keep the system's title bar.
 */
export function TitleBar() {
  const isFullscreen = useIsFullscreen();
  if (!isMac || isFullscreen) return null;
  return <div data-tauri-drag-region aria-hidden className="h-title-bar shrink-0 border-b border-line bg-bg" />;
}
