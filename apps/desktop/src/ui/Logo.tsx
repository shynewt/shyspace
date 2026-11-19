import { useId } from "react";

/** The shyspace mark: a shy planet. Matches `docs/brand/mark.svg`. */
export function Logo({ size = 28 }: { size?: number }) {
  // Ids are page-wide, and the logo can appear more than once.
  const id = useId();
  const fill = `${id}-fill`;
  const behindRing = `${id}-behind-ring`;
  const ringFront = "M3.4 16A12.6 4 0 0 0 28.6 16";
  return (
    <svg width={size} height={size} viewBox="0 0 32 32" aria-hidden="true">
      <defs>
        <linearGradient id={fill} x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" stopColor="#8b7fff" />
          <stop offset="1" stopColor="#5646f0" />
        </linearGradient>
        <mask id={behindRing} maskUnits="userSpaceOnUse" x="0" y="0" width="32" height="32">
          <rect width="32" height="32" fill="#fff" />
          <path d={ringFront} transform="rotate(-18 16 16)" fill="none" stroke="#000" strokeWidth="5.6" />
        </mask>
      </defs>
      <rect width="32" height="32" rx="9" fill={`url(#${fill})`} />
      <ellipse
        cx="16"
        cy="16"
        rx="12.6"
        ry="4"
        transform="rotate(-18 16 16)"
        fill="none"
        stroke="#dcd7ff"
        strokeWidth="2.8"
      />
      <g mask={`url(#${behindRing})`}>
        <circle cx="16" cy="16" r="8.6" fill="#fff" />
        <g transform="rotate(-14 16 16)">
          <ellipse cx="13" cy="14.2" rx="1.5" ry="1.5" fill="#3a2bbd" />
          <ellipse cx="19" cy="14.2" rx="1.5" ry="1.5" fill="#3a2bbd" />
          <ellipse cx="10" cy="15.6" rx="1.4" ry=".9" fill="#ff9cc8" />
          <ellipse cx="22" cy="15.6" rx="1.4" ry=".9" fill="#ff9cc8" />
        </g>
      </g>
      <path
        d={ringFront}
        transform="rotate(-18 16 16)"
        fill="none"
        stroke="#dcd7ff"
        strokeWidth="2.8"
        strokeLinecap="round"
      />
    </svg>
  );
}
