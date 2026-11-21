import { PanelLeft } from "lucide-react";
import {
  AnimatePresence,
  motion,
  type Transition,
  useReducedMotionConfig as useIsMotionReduced,
  useIsPresent,
} from "motion/react";
import { Dialog as RadixDialog } from "radix-ui";
import { createContext, type ReactNode, type RefObject, useContext, useEffect, useMemo, useRef } from "react";
import { layoutWidth, useElementWidth } from "../../lib/layout";
import { shortcutKeys } from "../../lib/shortcuts";
import { fade, IconButton, springs } from "../../ui";
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
  const regionTransition = useRegionTransition();

  return (
    <ShellLayoutContext.Provider value={value}>
      <motion.div
        className="flex h-full"
        initial={{ opacity: 0, scale: 0.985 }}
        animate={{ opacity: 1, scale: 1 }}
        transition={{ duration: 0.35, ease: [0.2, 0.8, 0.2, 1] }}
      >
        <AnimatePresence initial={false}>
          {sidebar.isDocked && (
            <motion.div
              key="sidebar"
              initial={{ width: 0 }}
              animate={{ width: "auto" }}
              exit={{ width: 0 }}
              transition={regionTransition}
              className="shrink-0 overflow-hidden"
            >
              <DockedSidebar>{sidebarContent}</DockedSidebar>
            </motion.div>
          )}
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

/** The docked sidebar; while it slides away it's inert, so nothing in it can be reached twice. */
function DockedSidebar({ children }: { children: ReactNode }) {
  const isPresent = useIsPresent();
  return (
    <aside
      aria-label="Sidebar"
      data-sidebar
      inert={!isPresent}
      aria-hidden={!isPresent || undefined}
      className="h-full w-sidebar-compact border-r border-line bg-bg sidebar-full:w-sidebar"
    >
      {children}
    </aside>
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
      <AnimatePresence>
        {isOpen && (
          <RadixDialog.Portal forceMount>
            <RadixDialog.Overlay asChild>
              <motion.div {...fade} className="fixed inset-0 z-30 bg-scrim backdrop-blur-[2px]" />
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
                animate={{ x: 0 }}
                exit={{ x: "-100%" }}
                transition={springs.move}
                data-sidebar
                className="fixed inset-y-0 left-0 z-30 w-sidebar max-w-[calc(100%-3rem)] border-r border-line bg-bg shadow-float outline-none"
              >
                <RadixDialog.Title className="sr-only">Sidebar</RadixDialog.Title>
                {children}
              </motion.div>
            </RadixDialog.Content>
          </RadixDialog.Portal>
        )}
      </AnimatePresence>
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

/** How regions grow and shrink: the panel spring, or at once with reduced motion (Motion itself only skips transforms). */
export function useRegionTransition(): Transition {
  return useIsMotionReduced() ? { duration: 0 } : springs.move;
}
