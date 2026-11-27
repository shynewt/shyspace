import { generate } from "lean-qr";
import { toSvgPath } from "lean-qr/extras/svg";
import { useMemo } from "react";

/** A QR code of `value`, drawn in the theme's ink on its surface, for a phone to scan. */
export function QrCode({ value, label, size = 176 }: { value: string; label: string; size?: number }) {
  const { path, modules } = useMemo(() => {
    const code = generate(value);
    return { path: toSvgPath(code), modules: code.size };
  }, [value]);
  // Four modules of quiet margin, as scanners expect.
  const margin = 4;
  return (
    <svg
      role="img"
      aria-label={label}
      width={size}
      height={size}
      viewBox={`${-margin} ${-margin} ${modules + 2 * margin} ${modules + 2 * margin}`}
      className="rounded-lg bg-white text-black"
      shapeRendering="crispEdges"
    >
      <path d={path} fill="currentColor" />
    </svg>
  );
}
