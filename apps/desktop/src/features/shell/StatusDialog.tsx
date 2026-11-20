import { Smile } from "lucide-react";
import { useRef, useState } from "react";
import { api, type MemberStatus, type NotifyLevel } from "../../lib/api";
import { formatTime } from "../../lib/people";
import { showToast } from "../../lib/toast";
import { useFocusOnMount } from "../../lib/useFocusOnMount";
import { useFormAction } from "../../lib/useFormAction";
import { Button, cx, Dialog, EmojiPicker, Field, FormError, Input, Popover, SegmentedControl } from "../../ui";

/** "unchanged" keeps the current status's end time, offered only while that's still ahead. */
type ClearAfter = "unchanged" | "never" | "hour" | "four_hours" | "today";

type Preset = { emoji: string; text: string; clearAfter: ClearAfter; alerts: NotifyLevel; detail: string };

const PRESETS: Preset[] = [
  { emoji: "📅", text: "In a meeting", clearAfter: "hour", alerts: "all", detail: "1 hour" },
  { emoji: "🎧", text: "Focusing", clearAfter: "four_hours", alerts: "mentions", detail: "4 hours, mentions only" },
  { emoji: "🍽️", text: "Lunch", clearAfter: "hour", alerts: "all", detail: "1 hour" },
  { emoji: "🤒", text: "Out sick", clearAfter: "today", alerts: "nothing", detail: "Today, no alerts" },
  { emoji: "🌴", text: "On holiday", clearAfter: "never", alerts: "nothing", detail: "No alerts" },
];

const HOUR_MS = 60 * 60 * 1000;

function clearTime(choice: ClearAfter, currentEnd: number | null, now: Date): number | null {
  if (choice === "unchanged") return currentEnd;
  if (choice === "hour") return now.getTime() + HOUR_MS;
  if (choice === "four_hours") return now.getTime() + 4 * HOUR_MS;
  if (choice === "today") {
    const endOfDay = new Date(now);
    endOfDay.setHours(23, 59, 59, 0);
    return endOfDay.getTime();
  }
  return null;
}

/** "17:00" for later today, "Sat 02:00" for another day. */
function describeEnd(ms: number): string {
  const isToday = new Date(ms).toDateString() === new Date().toDateString();
  return isToday ? formatTime(ms) : `${new Date(ms).toLocaleDateString([], { weekday: "short" })} ${formatTime(ms)}`;
}

/** Sets your status: what others see by your name, and how quiet your alerts are meanwhile. */
export function StatusDialog({
  open,
  onOpenChange,
  current,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  current: MemberStatus | null;
}) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange} title="Set a status" width={480}>
      <StatusForm current={current} onDone={() => onOpenChange(false)} />
    </Dialog>
  );
}

function StatusForm({ current, onDone }: { current: MemberStatus | null; onDone: () => void }) {
  const [emoji, setEmoji] = useState(current?.emoji ?? "");
  const [text, setText] = useState(current?.text ?? "");
  // Taken once: the status can end or change elsewhere while this is open.
  const [currentEnd] = useState(() => (current?.until_ms && current.until_ms > Date.now() ? current.until_ms : null));
  const [clearAfter, setClearAfter] = useState<ClearAfter>(currentEnd ? "unchanged" : "never");
  const [alerts, setAlerts] = useState<NotifyLevel>(current?.alerts ?? "all");
  const textInput = useRef<HTMLInputElement>(null);
  useFocusOnMount(textInput);
  const save = useFormAction(async () => {
    await api.setStatus(emoji.trim(), text.trim(), clearTime(clearAfter, currentEnd, new Date()), alerts);
    showToast("Status set.");
    onDone();
  });
  const choosePreset = (preset: Preset) => {
    setEmoji(preset.emoji);
    setText(preset.text);
    setClearAfter(preset.clearAfter);
    setAlerts(preset.alerts);
  };
  const clearChoices: Array<{ value: ClearAfter; label: string }> = [
    ...(currentEnd ? [{ value: "unchanged" as const, label: `Until ${describeEnd(currentEnd)}` }] : []),
    { value: "never", label: "Never" },
    { value: "hour", label: "1 hour" },
    { value: "four_hours", label: "4 hours" },
    { value: "today", label: "Today" },
  ];
  const isEmpty = !emoji.trim() && !text.trim() && alerts === "all";
  return (
    <form onSubmit={save.run} className="flex flex-col gap-4 p-5 pt-4">
      <div className="flex gap-2">
        <div className="flex shrink-0 flex-col gap-1.5">
          <span aria-hidden className="text-[12px] font-medium text-muted">
            Emoji
          </span>
          <EmojiChoice emoji={emoji} onChange={setEmoji} />
        </div>
        <div className="flex-1">
          <Field label="Status">
            {(id) => (
              <Input
                ref={textInput}
                id={id}
                value={text}
                onChange={(event) => setText(event.target.value)}
                placeholder="What are you up to?"
                maxLength={100}
              />
            )}
          </Field>
        </div>
      </div>
      <ul aria-label="Suggestions" className="-mx-1 flex flex-col">
        {PRESETS.map((preset) => (
          <li key={preset.text}>
            <button
              type="button"
              onClick={() => choosePreset(preset)}
              className={cx(
                "flex h-8 w-full items-center gap-2.5 rounded-lg px-2 text-left text-[13px] transition-colors hover:bg-surface-2",
                text === preset.text && emoji === preset.emoji && "bg-surface-2",
              )}
            >
              <span aria-hidden className="w-5 text-center">
                {preset.emoji}
              </span>
              <span className="font-medium">{preset.text}</span>
              <span className="text-muted">· {preset.detail}</span>
            </button>
          </li>
        ))}
      </ul>
      <Field label="Clear after">
        {() => (
          <SegmentedControl<ClearAfter>
            label="Clear after"
            options={clearChoices}
            value={clearAfter}
            onChange={setClearAfter}
          />
        )}
      </Field>
      <Field
        label="Alerts while it's set"
        hint="The quieter of this and a channel's own setting wins. Mentions only also lets through replies in threads you follow."
      >
        {() => (
          <SegmentedControl<NotifyLevel>
            label="Alerts while it's set"
            options={[
              { value: "all", label: "As usual" },
              { value: "mentions", label: "Mentions only" },
              { value: "nothing", label: "None" },
            ]}
            value={alerts}
            onChange={setAlerts}
          />
        )}
      </Field>
      <FormError message={save.error} />
      <div className="flex justify-end gap-2">
        <Button variant="ghost" onClick={onDone}>
          Cancel
        </Button>
        <Button type="submit" variant="primary" isLoading={save.isBusy} disabled={isEmpty}>
          Set status
        </Button>
      </div>
    </form>
  );
}

function EmojiChoice({ emoji, onChange }: { emoji: string; onChange: (emoji: string) => void }) {
  const [isOpen, setOpen] = useState(false);
  const choose = (next: string) => {
    onChange(next);
    setOpen(false);
  };
  return (
    <Popover
      open={isOpen}
      onOpenChange={setOpen}
      side="bottom"
      align="start"
      trigger={
        <button
          type="button"
          aria-label={emoji ? `Choose an emoji (now ${emoji})` : "Choose an emoji"}
          className="grid h-9 w-12 place-items-center rounded-lg border border-line bg-bg text-[18px] transition-colors hover:border-surface-3 data-[state=open]:border-accent"
        >
          {emoji || <Smile size={16} className="text-decorative" />}
        </button>
      }
    >
      <EmojiPicker onSelect={choose} />
      {emoji && (
        <div className="border-t border-line p-1">
          <Button variant="ghost" size="sm" onClick={() => choose("")} className="w-full">
            Remove emoji
          </Button>
        </div>
      )}
    </Popover>
  );
}
