/**
 * The layout widths that switch behaviour, read from the tokens in
 * `styles.css` so code and CSS never disagree. Everything else about the
 * layout is CSS (container queries on the regions).
 */
import { type RefObject, useLayoutEffect, useState, useSyncExternalStore } from "react";

type LayoutToken = "--breakpoint-sidebar-docks" | "--width-side-panel" | "--width-channel-min";

const widths = new Map<LayoutToken, number>();

/** A layout token's width in px. Throws if `styles.css` doesn't define it in px. */
export function layoutWidth(token: LayoutToken): number {
  const known = widths.get(token);
  if (known !== undefined) return known;
  const value = getComputedStyle(document.documentElement).getPropertyValue(token).trim();
  const width = Number.parseFloat(value);
  if (!value.endsWith("px") || Number.isNaN(width)) {
    throw new Error(`layout token ${token} is "${value}", not a width in px; see styles.css`);
  }
  // The tokens never change while the app runs.
  widths.set(token, width);
  return width;
}

const windowQueries = new Map<
  LayoutToken,
  { subscribe: (onChange: () => void) => () => void; matches: () => boolean }
>();

/** Whether the window is at least as wide as `token`, kept up to date. */
export function useIsWindowAtLeast(token: LayoutToken): boolean {
  let query = windowQueries.get(token);
  if (!query) {
    const media = window.matchMedia(`(min-width: ${layoutWidth(token)}px)`);
    query = {
      subscribe: (onChange) => {
        media.addEventListener("change", onChange);
        return () => media.removeEventListener("change", onChange);
      },
      matches: () => media.matches,
    };
    windowQueries.set(token, query);
  }
  return useSyncExternalStore(query.subscribe, query.matches);
}

/** An element's width in px, kept up to date; null until it's laid out. */
export function useElementWidth(ref: RefObject<HTMLElement | null>): number | null {
  const [width, setWidth] = useState<number | null>(null);
  useLayoutEffect(() => {
    const element = ref.current;
    if (!element) return;
    setWidth(element.clientWidth);
    const observer = new ResizeObserver(([entry]) => setWidth(entry.contentRect.width));
    observer.observe(element);
    return () => observer.disconnect();
  }, [ref]);
  return width;
}
