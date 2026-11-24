import { useId } from "react";

/** The colours a theme card draws for one side of a theme. */
export type ThemeSwatchColors = { bg: string; surface: string; border: string; text: string; muted: string };

export type ThemeSwatchOption<Value extends string> = {
  value: Value;
  label: string;
  /** Said after the label and shown small beside it, like "the workspace's default". */
  detail?: string;
  light: ThemeSwatchColors;
  dark: ThemeSwatchColors;
};

/**
 * Picks a theme from cards that show both its sides, light and dark. Native
 * radio buttons underneath, like SegmentedControl: Tab reaches the chosen
 * card and the arrow keys move it.
 */
export function ThemeSwatches<Value extends string>({
  label,
  options,
  value,
  onChange,
  disabled = false,
}: {
  label: string;
  options: ThemeSwatchOption<Value>[];
  value: Value;
  onChange: (value: Value) => void;
  disabled?: boolean;
}) {
  const groupName = useId();
  return (
    <div role="radiogroup" aria-label={label} className="grid grid-cols-3 gap-2.5">
      {options.map((option) => (
        <label key={option.value} className={disabled ? "relative" : "relative cursor-pointer"}>
          <input
            type="radio"
            name={groupName}
            value={option.value}
            aria-label={option.detail ? `${option.label}, ${option.detail}` : option.label}
            checked={option.value === value}
            disabled={disabled}
            onChange={() => onChange(option.value)}
            className="peer absolute inset-0 size-full cursor-[inherit] appearance-none rounded-lg"
          />
          <div
            aria-hidden
            className="pointer-events-none flex h-14 overflow-hidden rounded-lg border border-line ring-ink ring-offset-2 ring-offset-surface transition-shadow peer-checked:ring-2"
          >
            <ThemeSide colors={option.light} />
            <ThemeSide colors={option.dark} />
          </div>
          <div aria-hidden className="pointer-events-none mt-1 flex items-baseline gap-1.5 text-sm">
            {option.label}
            {option.detail && <span className="truncate text-2xs text-faint">{option.detail}</span>}
          </div>
        </label>
      ))}
    </div>
  );
}

/** A tiny window in one side's colours: a sidebar, a pane with two lines of text, and the accent. */
function ThemeSide({ colors }: { colors: ThemeSwatchColors }) {
  return (
    <div data-swatch-bg={colors.bg} className="flex h-full flex-1 gap-1 p-1.5" style={{ background: colors.bg }}>
      <div className="flex w-3 flex-col gap-1 pt-1">
        <span className="h-1 rounded-full" style={{ background: colors.muted }} />
        <span className="h-1 rounded-full" style={{ background: colors.muted, opacity: 0.6 }} />
      </div>
      <div
        className="flex flex-1 flex-col gap-1 rounded-[4px] border p-1.5"
        style={{ background: colors.surface, borderColor: colors.border }}
      >
        <span className="h-1 w-4/5 rounded-full" style={{ background: colors.text }} />
        <span className="h-1 w-3/5 rounded-full" style={{ background: colors.muted }} />
        <span className="mt-auto h-1.5 w-1/3 rounded-full bg-accent" />
      </div>
    </div>
  );
}
