import { motion } from "motion/react";
import { useId } from "react";
import { cx } from "./cx";
import { springs } from "./motion";

/** Picks one of a few options, with a pill that slides to the selection. */
export function SegmentedControl<Value extends string>({
  options,
  value,
  onChange,
}: {
  options: Array<{ value: Value; label: string }>;
  value: Value;
  onChange: (value: Value) => void;
}) {
  const pillId = useId();
  return (
    <div role="tablist" className="relative flex rounded-xl bg-surface-2 p-1">
      {options.map((option) => {
        const isSelected = option.value === value;
        return (
          <button
            key={option.value}
            type="button"
            role="tab"
            aria-selected={isSelected}
            onClick={() => onChange(option.value)}
            className={cx(
              "relative z-10 flex-1 rounded-lg py-2 text-[13px] font-medium transition-colors",
              isSelected ? "text-ink" : "text-muted hover:text-ink",
            )}
          >
            {isSelected && (
              <motion.span
                layoutId={pillId}
                transition={springs.snappy}
                className="absolute inset-0 -z-10 rounded-lg bg-surface shadow-card"
              />
            )}
            {option.label}
          </button>
        );
      })}
    </div>
  );
}
