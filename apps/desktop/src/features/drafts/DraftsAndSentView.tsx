import { Send } from "lucide-react";
import { Tabs } from "radix-ui";
import { type KeyboardEvent, type ReactNode, useEffect, useRef, useState } from "react";
import { hasCommandModifier } from "../../lib/shortcuts";
import { cx } from "../../ui";
import { ViewHeader } from "../shell/ViewHeader";
import { DraftList } from "./DraftList";
import { SentList } from "./SentList";

type Tab = "drafts" | "sent";
const TABS: Tab[] = ["drafts", "sent"];
const CHOSEN_TAB = '[role="tab"][aria-selected="true"]';
const ARROW_STOPS = `${CHOSEN_TAB}, input[type="search"], [data-list-row]`;

/** Everything you write: what you haven't sent yet, and what you sent with how people responded. */
export function DraftsAndSentView() {
  const [tab, setTab] = useState<Tab>("drafts");
  const tabList = useRef<HTMLDivElement>(null);
  // Opened on purpose, so the keyboard starts at the tabs.
  useEffect(() => {
    tabList.current?.querySelector<HTMLElement>(CHOSEN_TAB)?.focus();
  }, []);
  return (
    <div className="flex h-full flex-col">
      <ViewHeader>
        <Send size={17} className="shrink-0 text-decorative" />
        <h1 className="truncate text-lg font-semibold">Drafts & sent</h1>
      </ViewHeader>
      <Tabs.Root
        value={tab}
        onValueChange={(value) => {
          const chosen = TABS.find((candidate) => candidate === value);
          if (chosen) setTab(chosen);
        }}
        className="flex min-h-0 flex-1 flex-col"
        onKeyDown={moveWithArrows}
      >
        <div className="px-5 pt-3">
          <Tabs.List
            ref={tabList}
            aria-label="Drafts and sent"
            className="mx-auto flex max-w-reading gap-5 border-b border-line"
          >
            <TabTrigger value="drafts" isActive={tab === "drafts"}>
              Drafts
            </TabTrigger>
            <TabTrigger value="sent" isActive={tab === "sent"}>
              Sent
            </TabTrigger>
          </Tabs.List>
        </div>
        {TABS.map((value) => (
          <Tabs.Content
            key={value}
            value={value}
            tabIndex={-1}
            className="min-h-0 flex-1 overflow-y-auto px-5 py-4 outline-none"
          >
            <div className="mx-auto max-w-reading">{value === "drafts" ? <DraftList /> : <SentList />}</div>
          </Tabs.Content>
        ))}
      </Tabs.Root>
    </div>
  );
}

/** ↑ and ↓ step between the chosen tab, the filter field and the rows; with a modifier they're shortcuts. */
function moveWithArrows(event: KeyboardEvent<HTMLElement>) {
  if ((event.key !== "ArrowDown" && event.key !== "ArrowUp") || event.shiftKey || hasCommandModifier(event)) return;
  if (!(event.target instanceof HTMLElement)) return;
  const stops = [...event.currentTarget.querySelectorAll<HTMLElement>(ARROW_STOPS)];
  const index = stops.indexOf(event.target);
  const next = index === -1 ? undefined : stops[index + (event.key === "ArrowDown" ? 1 : -1)];
  if (!next) return;
  event.preventDefault();
  next.focus();
}

function TabTrigger({ value, isActive, children }: { value: Tab; isActive: boolean; children: ReactNode }) {
  return (
    <Tabs.Trigger
      value={value}
      className={cx(
        "-mb-px flex h-9 items-center gap-1.5 rounded-t-md border-b-2 text-sm -outline-offset-2 transition-colors",
        isActive ? "border-accent font-medium text-ink" : "border-transparent text-muted hover:text-ink",
      )}
    >
      {children}
    </Tabs.Trigger>
  );
}
