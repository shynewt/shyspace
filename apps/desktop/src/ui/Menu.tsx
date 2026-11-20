import { Check, ChevronRight } from "lucide-react";
import { motion } from "motion/react";
import { DropdownMenu } from "radix-ui";
import { Fragment, type ReactNode } from "react";
import { cx } from "./cx";
import { pop, springs } from "./motion";

export type MenuAction = {
  label: string;
  icon?: ReactNode;
  onSelect: () => void;
  isDestructive?: boolean;
};

/** A submenu of mutually exclusive choices; the chosen one is checked. */
export type MenuChoices = {
  label: string;
  icon?: ReactNode;
  choices: { label: string; icon?: ReactNode; isChosen: boolean; onSelect: () => void }[];
};

export type MenuItem = MenuAction | MenuChoices;

const ITEM =
  "flex h-8 cursor-default items-center gap-2 rounded-lg px-2.5 text-sm outline-none data-[highlighted]:bg-surface-2 data-[state=open]:bg-surface-2";
const SURFACE = "z-50 min-w-[180px] rounded-xl border border-line bg-surface p-1 shadow-float outline-none";

/**
 * A dropdown of actions. Keyboard navigation comes from Radix. Pass `open`
 * and `onOpenChange` to also open it from elsewhere, like a right-click.
 */
export function Menu({
  trigger,
  items,
  open,
  onOpenChange,
}: {
  trigger: ReactNode;
  items: MenuItem[];
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
}) {
  return (
    <DropdownMenu.Root open={open} onOpenChange={onOpenChange}>
      <DropdownMenu.Trigger asChild>{trigger}</DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content align="end" sideOffset={4} asChild>
          <motion.div {...pop} transition={springs.snappy} className={SURFACE}>
            {items.map((item, index) =>
              "choices" in item ? (
                <Choices key={item.label} item={item} />
              ) : (
                <Fragment key={item.label}>
                  {isFirstDestructive(items, index) && <DropdownMenu.Separator className="my-1 h-px bg-line" />}
                  <Action item={item} />
                </Fragment>
              ),
            )}
          </motion.div>
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
  );
}

/** Destructive actions sit apart, below a line. */
function isFirstDestructive(items: MenuItem[], index: number): boolean {
  const isDestructive = (item: MenuItem) => "isDestructive" in item && item.isDestructive === true;
  return index > 0 && isDestructive(items[index]) && !isDestructive(items[index - 1]);
}

function Action({ item }: { item: MenuAction }) {
  return (
    <DropdownMenu.Item
      onSelect={item.onSelect}
      className={cx(ITEM, item.isDestructive ? "text-danger-text" : "text-ink")}
    >
      {item.icon && <span className="text-muted">{item.icon}</span>}
      {item.label}
    </DropdownMenu.Item>
  );
}

function Choices({ item }: { item: MenuChoices }) {
  const chosen = item.choices.find((choice) => choice.isChosen)?.label;
  return (
    <DropdownMenu.Sub>
      <DropdownMenu.SubTrigger className={cx(ITEM, "text-ink")}>
        {item.icon && <span className="text-muted">{item.icon}</span>}
        <span className="flex-1">{item.label}</span>
        <ChevronRight size={14} className="text-decorative" />
      </DropdownMenu.SubTrigger>
      <DropdownMenu.Portal>
        <DropdownMenu.SubContent sideOffset={6} className={SURFACE}>
          <DropdownMenu.RadioGroup value={chosen}>
            {item.choices.map((choice) => (
              <DropdownMenu.RadioItem
                key={choice.label}
                value={choice.label}
                onSelect={choice.onSelect}
                className={cx(ITEM, "text-ink")}
              >
                {choice.icon && <span className="text-muted">{choice.icon}</span>}
                <span className="flex-1">{choice.label}</span>
                <DropdownMenu.ItemIndicator>
                  <Check size={14} className="text-accent" />
                </DropdownMenu.ItemIndicator>
              </DropdownMenu.RadioItem>
            ))}
          </DropdownMenu.RadioGroup>
        </DropdownMenu.SubContent>
      </DropdownMenu.Portal>
    </DropdownMenu.Sub>
  );
}
