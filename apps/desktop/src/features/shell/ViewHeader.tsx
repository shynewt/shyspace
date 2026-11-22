import { AnimatePresence, motion } from "motion/react";
import { type ComponentProps, type ReactNode, useRef } from "react";
import { Button, cx, springs, Tooltip } from "../../ui";
import { useMyHuddle } from "../huddles/HuddleContext";
import { HuddlePill } from "../huddles/HuddlePill";
import { SidebarToggle, useShellLayout } from "./ShellLayout";

/** The bar at the top of a view or panel: one line, with a rule below. */
export const HEADER_BAR = "flex h-13 shrink-0 items-center gap-2 border-b border-line";

/**
 * The bar at the top of a view (a channel, a page, Inbox, Later): one line.
 * While the sidebar isn't docked it starts with the sidebar toggle and ends
 * with your huddle, whose bar lives in the sidebar. It's the `header`
 * container, so what's in it can fold labels away as it narrows; the title
 * should be the only thing that shrinks.
 */
export function ViewHeader({ children, className }: { children?: ReactNode; className?: string }) {
  const { sidebar } = useShellLayout();
  const { mine } = useMyHuddle();
  return (
    <header className={cx("@container/header px-5", HEADER_BAR, className)}>
      <AnimatePresence initial={false}>
        {!sidebar.isDocked && <HeaderSidebarToggle key="sidebar-toggle" />}
      </AnimatePresence>
      <div className="flex min-w-0 flex-1 items-center gap-2">{children}</div>
      <AnimatePresence initial={false}>
        {!sidebar.isDocked && mine && <HuddlePill key="huddle" huddle={mine} />}
      </AnimatePresence>
    </header>
  );
}

/** Fades in as the sidebar goes, and goes at once as it comes back, since the header moves then anyway. */
function HeaderSidebarToggle() {
  return (
    <motion.div initial={{ opacity: 0 }} animate={{ opacity: 1 }} className="-ml-1.5 flex shrink-0">
      <SidebarToggle placement="header" />
    </motion.div>
  );
}

type HeaderActionProps = Omit<
  ComponentProps<typeof Button>,
  "children" | "variant" | "size" | "icon" | "isLoading" | "aria-label"
> & {
  icon: ReactNode;
  label: string;
  /** "button" is a ghost button; a pill is a small chip for information, `accent-pill` when it needs noticing. */
  look?: "button" | "pill" | "accent-pill";
};

const PILLS = {
  pill: "bg-surface-2 text-muted hover:text-ink",
  "accent-pill": "bg-accent-soft text-accent-text",
};

/**
 * A header button whose label folds away below the `header-labels` width,
 * leaving its icon, with the label as its tooltip and accessible name.
 */
export function HeaderAction({ icon, label, look = "button", className, ...props }: HeaderActionProps) {
  const text = useRef<HTMLSpanElement>(null);
  const labelText = (
    <span ref={text} className="@max-header-labels/header:hidden">
      {label}
    </span>
  );
  return (
    <Tooltip label={label} side="bottom" shouldShow={() => text.current?.getClientRects().length === 0}>
      {look === "button" ? (
        <Button
          variant="ghost"
          size="sm"
          icon={icon}
          aria-label={label}
          className={cx("@max-header-labels/header:w-7 @max-header-labels/header:px-0", className)}
          {...props}
        >
          {labelText}
        </Button>
      ) : (
        <motion.button
          type="button"
          aria-label={label}
          whileTap={{ scale: 0.95 }}
          transition={springs.snappy}
          className={cx(
            "flex h-6 shrink-0 items-center justify-center gap-1.5 rounded-full px-2.5 text-xs whitespace-nowrap transition-colors @max-header-labels/header:w-6 @max-header-labels/header:px-0",
            PILLS[look],
            className,
          )}
          {...props}
        >
          {icon}
          {labelText}
        </motion.button>
      )}
    </Tooltip>
  );
}
