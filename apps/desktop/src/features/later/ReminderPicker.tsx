import { AlarmClock } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import { useState } from "react";
import { describeReminderInFull, nextRoundHour, onDayAt, parseReminder, parseTimeOfDay } from "../../lib/reminders";
import { Button, Dialog, Field, fade, Input } from "../../ui";
import { Calendar } from "./Calendar";

/** Remind me at a time you pick: typed ("fri 3pm"), or a day and a time. Starts at `initialMs` if there is one. */
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
    <Dialog open={open} onOpenChange={onOpenChange} title="Remind me" width={360}>
      <PickerForm
        initialMs={initialMs}
        onCancel={() => onOpenChange(false)}
        onPick={(remindMs) => {
          onPick(remindMs);
          onOpenChange(false);
        }}
      />
    </Dialog>
  );
}

function timeText(at: Date): string {
  return `${String(at.getHours()).padStart(2, "0")}:${String(at.getMinutes()).padStart(2, "0")}`;
}

function PickerForm({
  initialMs,
  onCancel,
  onPick,
}: {
  initialMs: number | null;
  onCancel: () => void;
  onPick: (remindMs: number) => void;
}) {
  const [now] = useState(() => new Date());
  const [chosen, setChosen] = useState(() =>
    initialMs !== null && initialMs > now.getTime() ? new Date(initialMs) : nextRoundHour(now),
  );
  const [typed, setTyped] = useState("");
  const [time, setTime] = useState(() => timeText(chosen));
  const typedAt = typed.trim() ? parseReminder(typed, new Date()) : null;
  const typedTime = parseTimeOfDay(time);
  const at = typed.trim() ? typedAt : typedTime ? onDayAt(chosen, typedTime) : null;
  const isPast = at !== null && at.getTime() <= Date.now();
  const problem = (() => {
    if (typed.trim() && !typedAt) return "Try “tomorrow 9”, “fri 3pm” or “in 2h”.";
    if (!typed.trim() && !typedTime) return "Type a time like 15:00 or 3pm.";
    if (isPast) return "That time has already passed.";
    return null;
  })();

  const typeWhen = (text: string) => {
    setTyped(text);
    const parsed = parseReminder(text, new Date());
    if (!parsed) return;
    setChosen(parsed);
    setTime(timeText(parsed));
  };
  const pickDay = (day: Date) => {
    setTyped("");
    setChosen(typedTime ? onDayAt(day, typedTime) : day);
  };
  const submit = () => {
    if (at && !problem) onPick(at.getTime());
  };

  return (
    <form
      className="flex flex-col gap-4 p-5 pt-4"
      onSubmit={(event) => {
        event.preventDefault();
        submit();
      }}
    >
      <Field label="When" hint="Like “tomorrow 9”, “fri 3pm” or “in 2h”, or pick below.">
        {(id) => (
          <Input
            id={id}
            value={typed}
            autoFocus
            placeholder="tomorrow 9"
            onChange={(event) => typeWhen(event.target.value)}
          />
        )}
      </Field>
      <Calendar value={chosen} min={now} onChange={pickDay} />
      <Field label="Time">
        {(id) => (
          <Input
            id={id}
            value={time}
            inputMode="numeric"
            onChange={(event) => {
              setTyped("");
              setTime(event.target.value);
            }}
          />
        )}
      </Field>
      <div aria-live="polite" className="min-h-5 text-[12.5px]">
        <AnimatePresence mode="wait" initial={false}>
          <motion.p
            key={problem ?? at?.getTime() ?? "none"}
            {...fade}
            className={problem ? "text-warning-text" : "flex items-center gap-1.5 text-accent-text"}
          >
            {problem ?? (
              <>
                <AlarmClock size={13} /> {at && describeReminderInFull(at)}
              </>
            )}
          </motion.p>
        </AnimatePresence>
      </div>
      <div className="flex justify-end gap-2">
        <Button type="button" variant="ghost" onClick={onCancel}>
          Cancel
        </Button>
        <Button type="submit" variant="primary" disabled={problem !== null}>
          Set reminder
        </Button>
      </div>
    </form>
  );
}
