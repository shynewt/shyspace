import { AnimatePresence, motion } from "motion/react";
import { Switch } from "radix-ui";
import { useId, useRef } from "react";
import type { WorkingHours } from "../../lib/api";
import { formatMinuteOfDay, MINUTES_PER_DAY, weekdayName } from "../../lib/localTime";
import { cx, Select, springs } from "../../ui";

/** What turning working hours on suggests: Monday to Friday, 09:00 to 17:00. */
export const SUGGESTED_HOURS: WorkingHours = { days: 0b001_1111, start_minute: 9 * 60, end_minute: 17 * 60 };
const STEP_MINUTES = 30;
/** Monday (0) to Sunday (6). */
const WEEKDAYS = [0, 1, 2, 3, 4, 5, 6];

/** Why these hours can't be saved, or null. */
export function workingHoursProblem(hours: WorkingHours | null): string | null {
  if (!hours) return null;
  if (hours.days === 0) return "Pick at least one day.";
  if (hours.start_minute === hours.end_minute) return "Start and end can't be the same time.";
  return null;
}

/** Every half hour from `first` to `last`, plus `current` if it falls between them (set elsewhere). */
function timeOptions(current: number, first: number, last: number): number[] {
  const steps = Array.from({ length: (last - first) / STEP_MINUTES + 1 }, (_, index) => first + index * STEP_MINUTES);
  return steps.includes(current) ? steps : [...steps, current].sort((left, right) => left - right);
}

/** How an end time reads: midnight, or a time on the next day when it's before the start. */
function describeEnd(minute: number, start: number): string {
  if (minute === MINUTES_PER_DAY) return `${formatMinuteOfDay(0)} (midnight)`;
  return `${formatMinuteOfDay(minute)}${minute <= start ? " (next day)" : ""}`;
}

/**
 * Whether to share working hours, and which: the days, then a start and an
 * end. An end before the start runs past midnight. Turning sharing off and
 * on again brings back the hours you had.
 */
export function WorkingHoursField({
  value,
  onChange,
  error,
}: {
  value: WorkingHours | null;
  onChange: (hours: WorkingHours | null) => void;
  error: string | null;
}) {
  const switchId = useId();
  const errorId = useId();
  const lastShared = useRef(value ?? SUGGESTED_HOURS);
  if (value) lastShared.current = value;
  const change = (part: Partial<WorkingHours>) => value && onChange({ ...value, ...part });

  return (
    <div className="flex flex-col gap-2.5">
      <div className="flex items-center gap-3">
        <Switch.Root
          id={switchId}
          checked={value !== null}
          onCheckedChange={(isOn) => onChange(isOn ? lastShared.current : null)}
          className="relative h-5 w-9 shrink-0 rounded-full bg-surface-3 transition-colors data-[state=checked]:bg-accent"
        >
          <Switch.Thumb asChild>
            <motion.span
              animate={{ x: value ? 18 : 2 }}
              initial={false}
              transition={springs.snappy}
              className="block size-4 rounded-full bg-surface shadow-card"
            />
          </Switch.Thumb>
        </Switch.Root>
        <label htmlFor={switchId} className="text-sm">
          Share working hours
        </label>
      </div>
      <AnimatePresence initial={false}>
        {value && (
          <motion.div key="hours" initial={{ opacity: 0 }} animate={{ opacity: 1 }}>
            <fieldset
              data-field="working_hours"
              tabIndex={-1}
              aria-describedby={error ? errorId : undefined}
              className="flex flex-col gap-2.5 pt-0.5 outline-none"
            >
              <legend className="sr-only">Working hours</legend>
              <fieldset className="flex gap-1">
                <legend className="sr-only">Working days</legend>
                {WEEKDAYS.map((weekday) => {
                  const isOn = (value.days & (1 << weekday)) !== 0;
                  return (
                    <button
                      key={weekday}
                      type="button"
                      aria-pressed={isOn}
                      aria-label={weekdayName(weekday, "long")}
                      onClick={() => change({ days: value.days ^ (1 << weekday) })}
                      className={cx(
                        "h-8 w-11 rounded-lg border text-sm transition-colors",
                        isOn
                          ? "border-accent bg-accent-soft font-medium text-accent-text"
                          : "border-line text-muted hover:bg-surface-2 hover:text-ink",
                      )}
                    >
                      {weekdayName(weekday, "short")}
                    </button>
                  );
                })}
              </fieldset>
              <div className="flex items-center gap-2 text-sm text-muted">
                <Select
                  aria-label="Start"
                  value={String(value.start_minute)}
                  options={timeOptions(value.start_minute, 0, MINUTES_PER_DAY - STEP_MINUTES).map((minute) => ({
                    value: String(minute),
                    label: formatMinuteOfDay(minute),
                  }))}
                  onChange={(minute) => change({ start_minute: Number(minute) })}
                  className="w-32"
                />
                <span aria-hidden>to</span>
                <Select
                  aria-label="End"
                  value={String(value.end_minute)}
                  options={timeOptions(value.end_minute, STEP_MINUTES, MINUTES_PER_DAY).map((minute) => ({
                    value: String(minute),
                    label: describeEnd(minute, value.start_minute),
                  }))}
                  onChange={(minute) => change({ end_minute: Number(minute) })}
                  className="w-44"
                />
              </div>
              {error && (
                <span id={errorId} className="text-xs text-danger-text">
                  {error}
                </span>
              )}
            </fieldset>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}
