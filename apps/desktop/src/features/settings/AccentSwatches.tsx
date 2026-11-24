import { useId } from "react";
import { ACCENTS, type Accent } from "../../lib/preferences";
import { Tooltip } from "../../ui";

const ACCENT_NAMES: Record<Accent, string> = {
  violet: "Violet",
  blue: "Blue",
  indigo: "Indigo",
  teal: "Teal",
  pink: "Pink",
  graphite: "Graphite",
};

/**
 * Picks the accent colour. Native radio buttons underneath, like
 * SegmentedControl: Tab reaches the chosen swatch and the arrow keys move it.
 */
export function AccentSwatches({
  label = "Accent colour",
  value,
  onChange,
  disabled = false,
}: {
  label?: string;
  value: Accent;
  onChange: (accent: Accent) => void;
  disabled?: boolean;
}) {
  const groupName = useId();
  return (
    <div role="radiogroup" aria-label={label} className="flex flex-wrap gap-1.5">
      {ACCENTS.map((accent) => (
        <Tooltip key={accent} label={ACCENT_NAMES[accent]}>
          <label className="relative grid size-8 place-items-center rounded-full">
            <input
              type="radio"
              name={groupName}
              value={accent}
              aria-label={ACCENT_NAMES[accent]}
              checked={accent === value}
              disabled={disabled}
              onChange={() => onChange(accent)}
              className="peer absolute inset-0 cursor-pointer appearance-none rounded-full disabled:cursor-default"
            />
            <span
              aria-hidden
              data-accent={accent}
              className="pointer-events-none size-6 rounded-full bg-accent ring-ink ring-offset-2 ring-offset-surface transition-shadow peer-checked:ring-2"
            />
          </label>
        </Tooltip>
      ))}
    </div>
  );
}
