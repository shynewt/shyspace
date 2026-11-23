import { PanelLeft } from "lucide-react";
import { AnimatePresence, motion, useIsPresent } from "motion/react";
import { Dialog as RadixDialog } from "radix-ui";
import { createContext, type ReactNode, type RefObject, useContext, useEffect, useMemo, useRef } from "react";
import { layoutWidth, useElementWidth } from "../../lib/layout";
import { shortcutKeys } from "../../lib/shortcuts";
import { cx, fade, IconButton, instantAfterAKey, springs, tweens } from "../../ui";
import type { SidebarState } from "./useSidebar";

type ShellLayoutValue = {
  sidebar: SidebarState;
  /** Whether a side panel (a thread) fits beside the content, or slides over it. */
  canDockSidePanel: boolean;
};

const ShellLayoutContext = createContext<ShellLayoutValue | null>(null);

export function useShellLayout(): ShellLayoutValue {
  const value = useContext(ShellLayoutContext);
  if (!value) throw new Error("useShellLayout needs a ShellLayout around it");
  return value;
}

/**
 * The window's regions: the sidebar (docked, hidden, or a drawer in a narrow
 * window) and the main column, where views put their own side panels.
 */
export function ShellLayout({
  sidebar,
  sidebarContent,
  children,
}: {
  sidebar: SidebarState;
  sidebarContent: ReactNode;
  children: ReactNode;
}) {
  const main = useRef<HTMLElement>(null);
  // Before the first layout, the window's width is the best guess (a panel open at launch mustn't slide in).
  const mainWidth = useElementWidth(main) ?? window.innerWidth;
  const canDockSidePanel = mainWidth >= layoutWidth("--width-side-panel") + layoutWidth("--width-channel-min");
  const value = useMemo(() => ({ sidebar, canDockSidePanel }), [sidebar, canDockSidePanel]);

  return (
    <ShellLayoutContext.Provider value={value}>
      <motion.div className="relative flex h-full" initial={{ opacity: 0 }} animate={{ opacity: 1 }}>
        <AnimatePresence initial={false}>
          {sidebar.isDocked && <DockedSidebar key="sidebar">{sidebarContent}</DockedSidebar>}
        </AnimatePresence>
        <main ref={main} tabIndex={-1} className="@container/main relative min-w-0 flex-1 bg-surface outline-none">
          {children}
        </main>
      </motion.div>
      <SidebarDrawer sidebar={sidebar} main={main}>
        {sidebarContent}
      </SidebarDrawer>
    </ShellLayoutContext.Provider>
  );
}

/**
 * The docked sidebar. It slides in and out by transform; while it slides away
 * it leaves the layout, so the content takes its new width at once instead of
 * on every frame, and it's inert, so nothing in it can be reached twice.
 */
function DockedSidebar({ children }: { children: ReactNode }) {
  const isPresent = useIsPresent();
  return (
    <motion.aside
      aria-label="Sidebar"
      data-sidebar
      inert={!isPresent}
      aria-hidden={!isPresent || undefined}
      initial={{ x: "-100%" }}
      animate={{ x: 0, transition: instantAfterAKey(springs.move) }}
      exit={{ x: "-100%", transition: instantAfterAKey(tweens.exit) }}
      className={cx(
        "h-full w-sidebar-compact shrink-0 border-r border-line bg-bg sidebar-full:w-sidebar",
        !isPresent && "absolute inset-y-0 left-0 z-20",
      )}
    >
      {children}
    </motion.aside>
  );
}

/** The sidebar sliding over the content in a narrow window: a modal, so focus moves in and back like a dialog's. */
function SidebarDrawer({
  sidebar,
  main,
  children,
}: {
  sidebar: SidebarState;
  main: RefObject<HTMLElement | null>;
  children: ReactNode;
}) {
  const isOpen = sidebar.drawer === "open";
  // Radix hands focus back to a Dialog.Trigger; the toggles and the shortcut aren't one.
  const opener = useRef<Element | null>(null);
  // Read as the drawer finishes closing, so the latest state, not that of its last render.
  const isLeaving = useRef(false);
  useEffect(() => {
    isLeaving.current = sidebar.drawer === "left";
  }, [sidebar.drawer]);
  return (
    <RadixDialog.Root open={isOpen} onOpenChange={(open) => !open && sidebar.closeDrawer()}>
      {/* No exit animation: one that never finished would leave the drawer blocking clicks. */}
      <RadixDialog.Portal>
        <RadixDialog.Overlay asChild>
          <motion.div {...fade} className="fixed inset-0 z-30 bg-scrim" />
        </RadixDialog.Overlay>
        <RadixDialog.Content
          asChild
          aria-describedby={undefined}
          onOpenAutoFocus={() => {
            opener.current = document.activeElement;
          }}
          onCloseAutoFocus={(event) => {
            event.preventDefault();
            const focused = document.activeElement;
            const isFocusLost = !focused || focused === document.body;
            const back = opener.current;
            // An opener that's gone or going (the header toggle, once the sidebar docks) can't take focus.
            if (!isLeaving.current && back instanceof HTMLElement && back.isConnected && !back.closest("[inert]")) {
              back.focus();
            } else if (isFocusLost) {
              // Where you went keeps focus (a channel focuses its message box); otherwise the content takes it.
              main.current?.focus({ preventScroll: true });
            }
          }}
        >
          <motion.div
            initial={{ x: "-100%" }}
            animate={{ x: 0, transition: instantAfterAKey(springs.move) }}
            data-sidebar
            className="fixed inset-y-0 left-0 z-30 w-sidebar max-w-[calc(100%-3rem)] border-r border-line bg-bg shadow-float outline-none"
          >
            <RadixDialog.Title className="sr-only">Sidebar</RadixDialog.Title>
            {children}
          </motion.div>
        </RadixDialog.Content>
      </RadixDialog.Portal>
    </RadixDialog.Root>
  );
}

/**
 * The button that shows or hides the sidebar: in the sidebar's top row while
 * it's there, at the start of the view's header while it isn't.
 */
export function SidebarToggle({ placement }: { placement: "sidebar" | "header" }) {
  const { sidebar } = useShellLayout();
  const button = useRef<HTMLButtonElement>(null);
  const { claimToggleFocus } = sidebar;
  useEffect(() => {
    if (claimToggleFocus()) button.current?.focus();
  }, [claimToggleFocus]);
  const label = placement === "header" ? "Show sidebar" : sidebar.mode === "drawer" ? "Close sidebar" : "Hide sidebar";
  return (
    <IconButton
      ref={button}
      label={label}
      shortcut={shortcutKeys("toggleSidebar")}
      icon={<PanelLeft size={16} />}
      onClick={sidebar.toggle}
      data-sidebar-toggle
    />
  );
}
