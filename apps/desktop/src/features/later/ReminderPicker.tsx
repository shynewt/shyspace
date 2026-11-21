import { AlarmClock, CalendarDays, CornerDownLeft } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import { type ReactNode, useId, useState } from "react";
import { formatTime } from "../../lib/people";
import {
  describeDistance,
  describeReminderInFull,
  nextRoundHour,
  onDayAt,
  parseTimeOfDay,
  reminderReadings,
} from "../../lib/reminders";
import { Button, cx, Dialog, durations, fade, Input, springs } from "../../ui";
import { Calendar } from "./Calendar";

const EXAMPLES = "“in 5 min”, “tomorrow 9” or “fri 3pm”";

/** Grows or shrinks to fit as it comes and goes, so the dialog's size changes smoothly. */
const unfold = {
  initial: { opacity: 0, height: 0 },
  animate: { opacity: 1, height: "auto" },
  exit: { opacity: 0, height: 0 },
  transition: { ...springs.smooth, opacity: { duration: durations.base } },
} as const;

/**
 * Remind me at a time you type ("5min", "fri 3pm") and choose from what it
 * could mean, or at a day and time picked on a calendar. Shows `initialMs` if
 * there is one.
 */
export function ReminderPicker({
  open,
  onOpenChange,
  initialMs,
  onPick,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  initialMs: number | null;
  onPick: (remindMs: number) => void;
}) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange} title="Remind me" hideTitle padding="none" width={400}>
      <Picker
        initialMs={initialMs}
        onPick={(remindMs) => {
          onPick(remindMs);
          onOpenChange(false);
        }}
      />
    </Dialog>
  );
}

type Option = { key: string; label: string; detail?: string; icon?: ReactNode; onChoose: () => void };

function Picker({ initialMs, onPick }: { initialMs: number | null; onPick: (remindMs: number) => void }) {
  const [now, setNow] = useState(() => new Date());
  const current = initialMs !== null && initialMs > now.getTime() ? new Date(initialMs) : null;
  const [typed, setTyped] = useState("");
  const [selected, setSelected] = useState(0);
  const [calendarStart, setCalendarStart] = useState<Date | null>(null);
  const listId = useId();

  const readings = reminderReadings(typed, now);
  const upcoming = readings.filter((at) => at > now);
  const choices = upcoming.map((at) => ({
    key: describeReminderInFull(at, now),
    label: describeReminderInFull(at, now),
    detail: describeDistance(at, now),
    onChoose: () => onPick(at.getTime()),
  }));
  const options: Option[] = [
    ...choices.filter((choice, index) => choices.findIndex((other) => other.key === choice.key) === index),
    {
      key: "calendar",
      label: "Pick a date and time…",
      icon: <CalendarDays size={15} />,
      onChoose: () => setCalendarStart(upcoming[0] ?? current ?? nextRoundHour(now)),
    },
  ];
  const active = Math.min(selected, options.length - 1);
  const hasPassed = typed.trim() !== "" && upcoming.length === 0 && readings.length > 0;
  const message = (() => {
    if (!typed.trim())
      return current ? `Set for ${describeReminderInFull(current, now)}` : `Type when, like ${EXAMPLES}.`;
    if (upcoming.length > 0) return null;
    return hasPassed ? "That time has already passed." : `No time matches that. Try ${EXAMPLES}.`;
  })();

  const type = (text: string) => {
    setTyped(text);
    setNow(new Date());
    setSelected(0);
    setCalendarStart(null);
  };

  return (
    <>
      <div className="flex items-center gap-3 px-4">
        <AlarmClock size={17} className="shrink-0 text-decorative" />
        <input
          value={typed}
          aria-label="When"
          role="combobox"
          aria-expanded={calendarStart === null}
          aria-controls={listId}
          aria-activedescendant={calendarStart === null ? `${listId}-${active}` : undefined}
          onChange={(event) => type(event.target.value)}
          onKeyDown={(event) => {
            if (calendarStart !== null) return;
            if (event.key === "ArrowDown") {
              event.preventDefault();
              setSelected(Math.min(active + 1, options.length - 1));
            } else if (event.key === "ArrowUp") {
              event.preventDefault();
              setSelected(Math.max(active - 1, 0));
            } else if (event.key === "Enter") {
              event.preventDefault();
              options[active].onChoose();
            }
          }}
          placeholder="Remind me in 5 min, tomorrow 9…"
          className="h-13 min-w-0 flex-1 bg-transparent text-lg outline-none placeholder:text-faint"
        />
      </div>
      <AnimatePresence initial={false} mode="wait">
        {calendarStart === null ? (
          <motion.div key="typed" {...unfold} className="overflow-hidden">
            <div className="border-t border-line p-2">
              <AnimatePresence initial={false} mode="popLayout">
                {message && (
                  <motion.p
                    key={message}
                    {...fade}
                    aria-live="polite"
                    className={cx("px-3 pt-1.5 pb-2 text-sm", hasPassed ? "text-warning-text" : "text-muted")}
                  >
                    {message}
                  </motion.p>
                )}
              </AnimatePresence>
              <div id={listId} role="listbox" aria-label="Suggested times">
                <AnimatePresence initial={false}>
                  {options.map((option, index) => (
                    <motion.div key={option.key} {...unfold} className="overflow-hidden">
                      <OptionRow
                        id={`${listId}-${index}`}
                        option={option}
                        isSelected={index === active}
                        onHover={() => setSelected(index)}
                      />
                    </motion.div>
                  ))}
                </AnimatePresence>
              </div>
            </div>
          </motion.div>
        ) : (
          <motion.div key="calendar" {...unfold} className="overflow-hidden">
            <CalendarChoice start={calendarStart} now={now} onPick={onPick} />
          </motion.div>
        )}
      </AnimatePresence>
    </>
  );
}

function OptionRow({
  id,
  option,
  isSelected,
  onHover,
}: {
  id: string;
  option: Option;
  isSelected: boolean;
  onHover: () => void;
}) {
  return (
    <button
      id={id}
      type="button"
      role="option"
      tabIndex={-1}
      aria-selected={isSelected}
      onMouseMove={onHover}
      onClick={option.onChoose}
      className="relative flex w-full items-center gap-3 rounded-lg px-3 py-2 text-left"
    >
      {isSelected && (
        <motion.span
          layoutId="reminder-option"
          transition={springs.snappy}
          className="absolute inset-0 rounded-lg bg-surface-2"
        />
      )}
      {option.icon && (
        <span className={cx("relative", isSelected ? "text-accent" : "text-decorative")}>{option.icon}</span>
      )}
      <span className="relative min-w-0 flex-1 truncate text-base">{option.label}</span>
      {option.detail && <span className="relative shrink-0 text-sm text-muted tabular-nums">{option.detail}</span>}
      <CornerDownLeft size={14} className={cx("relative shrink-0 text-decorative", !isSelected && "invisible")} />
    </button>
  );
}

/** A clock time `parseTimeOfDay` reads back: the local format when it can, else 24-hour. */
function editableTime(at: Date): string {
  const local = formatTime(at.getTime());
  if (parseTimeOfDay(local)) return local;
  return `${String(at.getHours()).padStart(2, "0")}:${String(at.getMinutes()).padStart(2, "0")}`;
}

function CalendarChoice({ start, now, onPick }: { start: Date; now: Date; onPick: (remindMs: number) => void }) {
  const [day, setDay] = useState(start);
  const [time, setTime] = useState(() => editableTime(start));
  const timeId = useId();
  const clock = parseTimeOfDay(time);
  const at = clock ? onDayAt(day, clock) : null;
  const problem = (() => {
    if (!at) return "Type a time like 3pm or 15:00.";
    if (at.getTime() <= Date.now()) return "That time has already passed.";
    return null;
  })();

  return (
    <form
      className="flex flex-col gap-3 border-t border-line px-4 pt-3 pb-4"
      onSubmit={(event) => {
        event.preventDefault();
        if (at && !problem) onPick(at.getTime());
      }}
    >
      <Calendar value={day} min={now} onChange={setDay} />
      <div className="flex items-center gap-2">
        <label htmlFor={timeId} className="text-sm text-muted">
          Time
        </label>
        <Input id={timeId} value={time} onChange={(event) => setTime(event.target.value)} className="w-28" />
        <span className="flex-1" />
        <Button type="submit" variant="primary" disabled={problem !== null}>
          Set reminder
        </Button>
      </div>
      <p aria-live="polite" className={cx("min-h-5 text-sm", problem ? "text-warning-text" : "text-muted")}>
        {problem ?? (at && describeReminderInFull(at, now))}
      </p>
    </form>
  );
}
