import { motion } from "motion/react";
import { useId } from "react";
import { cx } from "./cx";
import { springs } from "./motion";

/**
 * Picks one of a few options. Native radio buttons underneath: Tab reaches
 * the chosen option and the arrow keys move the choice. Sized to its
 * options unless `width` is "full".
 */
export function SegmentedControl<Value extends string>({
  label,
  options,
  value,
  onChange,
  width = "content",
}: {
  /** The group's accessible name, usually the visible label beside it. */
  label: string;
  options: Array<{ value: Value; label: string }>;
  value: Value;
  onChange: (value: Value) => void;
  width?: "content" | "full";
}) {
  const groupName = useId();
  return (
    <div
      role="radiogroup"
      aria-label={label}
      className={cx(
        "relative gap-0.5 rounded-xl border border-line bg-surface-2 p-0.5",
        width === "full" ? "flex" : "inline-flex max-w-full self-start",
      )}
    >
      {options.map((option) => {
        const isSelected = option.value === value;
        return (
          <label
            key={option.value}
            className={cx(
              "relative z-10 grid h-8 place-items-center rounded-[10px] px-3 text-[13px] whitespace-nowrap transition-colors",
              width === "full" && "flex-1",
              isSelected ? "font-semibold text-accent-text" : "font-medium text-muted hover:text-ink",
            )}
          >
            <input
              type="radio"
              name={groupName}
              value={option.value}
              checked={isSelected}
              onChange={() => onChange(option.value)}
              className="absolute inset-0 cursor-pointer appearance-none rounded-[10px]"
            />
            {isSelected && (
              <motion.span
                layoutId={groupName}
                transition={springs.snappy}
                className="absolute inset-0 -z-10 rounded-[10px] bg-accent-soft shadow-card ring-1 ring-accent/35 ring-inset"
              />
            )}
            <span className="pointer-events-none relative">{option.label}</span>
          </label>
        );
      })}
    </div>
  );
}
