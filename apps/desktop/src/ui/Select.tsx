import { Check, ChevronDown } from "lucide-react";
import { motion } from "motion/react";
import { Select as RadixSelect } from "radix-ui";
import type { ReactNode } from "react";
import { cx } from "./cx";
import { MENU_ITEM, MENU_SURFACE } from "./Menu";
import { pop } from "./motion";

export type SelectOption<Value extends string> = { value: Value; label: string; icon?: ReactNode };

/**
 * A choice from a list, drawn by the app rather than the system (whose list
 * can't be styled in the app's webview), so it matches `Menu`. Keyboard use
 * and typeahead come from Radix. Label it with a `Field` (pass its `id`) or
 * with `aria-label`.
 */
export function Select<Value extends string>({
  value,
  options,
  onChange,
  id,
  className,
  "aria-label": ariaLabel,
  "aria-describedby": ariaDescribedBy,
}: {
  value: Value;
  options: SelectOption<Value>[];
  onChange: (value: Value) => void;
  id?: string;
  /** Sets the width; the list is at least as wide as the button. */
  className?: string;
  "aria-label"?: string;
  "aria-describedby"?: string;
}) {
  const chosen = options.find((option) => option.value === value);
  return (
    <RadixSelect.Root
      value={value}
      onValueChange={(next) => {
        const option = options.find((candidate) => candidate.value === next);
        if (option) onChange(option.value);
      }}
    >
      <RadixSelect.Trigger
        id={id}
        aria-label={ariaLabel}
        aria-describedby={ariaDescribedBy}
        className={cx(
          "flex h-9 items-center gap-2 rounded-lg border border-line bg-bg px-2.5 text-left text-sm text-ink outline-none transition hover:bg-surface-2 focus-visible:border-accent focus-visible:ring-3 focus-visible:ring-accent/15 disabled:opacity-50 data-[state=open]:border-accent",
          className,
        )}
      >
        {chosen?.icon && <span className="flex shrink-0 text-muted">{chosen.icon}</span>}
        <span className="min-w-0 flex-1 truncate">
          <RadixSelect.Value>{chosen?.label}</RadixSelect.Value>
        </span>
        <RadixSelect.Icon asChild>
          <ChevronDown size={14} className="shrink-0 text-decorative" />
        </RadixSelect.Icon>
      </RadixSelect.Trigger>
      <RadixSelect.Portal>
        <RadixSelect.Content position="popper" sideOffset={4} asChild>
          <motion.div
            {...pop}
            className={cx(
              MENU_SURFACE,
              "max-h-(--radix-select-content-available-height) min-w-(--radix-select-trigger-width) origin-(--radix-select-content-transform-origin) overflow-hidden p-0",
            )}
          >
            <RadixSelect.Viewport className="p-1">
              {options.map((option) => (
                <RadixSelect.Item key={option.value} value={option.value} className={cx(MENU_ITEM, "text-ink")}>
                  {option.icon && <span className="flex shrink-0 text-muted">{option.icon}</span>}
                  <RadixSelect.ItemText>{option.label}</RadixSelect.ItemText>
                  <RadixSelect.ItemIndicator className="ml-auto pl-2">
                    <Check size={14} className="text-accent" />
                  </RadixSelect.ItemIndicator>
                </RadixSelect.Item>
              ))}
            </RadixSelect.Viewport>
          </motion.div>
        </RadixSelect.Content>
      </RadixSelect.Portal>
    </RadixSelect.Root>
  );
}
