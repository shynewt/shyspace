import { motion } from "motion/react";
import { Switch as RadixSwitch } from "radix-ui";
import { useId } from "react";
import { springs } from "./motion";

/** An on/off setting with its label beside it; the label toggles it too. */
export function Switch({
  label,
  checked,
  onChange,
}: {
  label: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
}) {
  const id = useId();
  return (
    <div className="flex items-center gap-3">
      <RadixSwitch.Root
        id={id}
        checked={checked}
        onCheckedChange={onChange}
        className="relative h-5 w-9 shrink-0 rounded-full bg-surface-3 transition-colors data-[state=checked]:bg-accent"
      >
        <RadixSwitch.Thumb asChild>
          <motion.span
            animate={{ x: checked ? 18 : 2 }}
            initial={false}
            transition={springs.snappy}
            className="block size-4 rounded-full bg-surface shadow-card"
          />
        </RadixSwitch.Thumb>
      </RadixSwitch.Root>
      <label htmlFor={id} className="text-sm">
        {label}
      </label>
    </div>
  );
}
