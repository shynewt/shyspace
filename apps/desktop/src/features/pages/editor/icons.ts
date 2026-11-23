/** Lucide icons for editor widgets, which are plain DOM, not React. */

export function lucideIcon(paths: readonly string[]): SVGSVGElement {
  const namespace = "http://www.w3.org/2000/svg";
  const icon = document.createElementNS(namespace, "svg");
  for (const [name, value] of Object.entries({
    viewBox: "0 0 24 24",
    width: "0.95em",
    height: "0.95em",
    fill: "none",
    stroke: "currentColor",
    "stroke-width": "2",
    "stroke-linecap": "round",
    "stroke-linejoin": "round",
    "aria-hidden": "true",
  })) {
    icon.setAttribute(name, value);
  }
  for (const path of paths) {
    const line = document.createElementNS(namespace, "path");
    line.setAttribute("d", path);
    icon.append(line);
  }
  return icon;
}

/** Lucide's "file". */
export function fileIcon(): SVGSVGElement {
  return lucideIcon(["M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z", "M14 2v4a2 2 0 0 0 2 2h4"]);
}

/** Lucide's "quote", as on message cards in chat. */
export function messageIcon(): SVGSVGElement {
  return lucideIcon([
    "M16 3a2 2 0 0 0-2 2v6a2 2 0 0 0 2 2 1 1 0 0 1 1 1v1a2 2 0 0 1-2 2 1 1 0 0 0-1 1v2a1 1 0 0 0 1 1 6 6 0 0 0 6-6V5a2 2 0 0 0-2-2z",
    "M5 3a2 2 0 0 0-2 2v6a2 2 0 0 0 2 2 1 1 0 0 1 1 1v1a2 2 0 0 1-2 2 1 1 0 0 0-1 1v2a1 1 0 0 0 1 1 6 6 0 0 0 6-6V5a2 2 0 0 0-2-2z",
  ]);
}

/** Lucide's "file-text". */
export function pageIcon(): SVGSVGElement {
  return lucideIcon([
    "M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z",
    "M14 2v4a2 2 0 0 0 2 2h4",
    "M10 9H8",
    "M16 13H8",
    "M16 17H8",
  ]);
}
