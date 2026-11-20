import { ChevronLeft, ChevronRight } from "lucide-react";
import { AnimatePresence, motion, useIsPresent } from "motion/react";
import { type KeyboardEvent, type RefObject, useEffect, useRef, useState } from "react";
import { cx, IconButton, springs } from "../../ui";

const WEEK = 7;

function startOfDay(date: Date): Date {
  return new Date(date.getFullYear(), date.getMonth(), date.getDate());
}

function addDays(date: Date, days: number): Date {
  return new Date(date.getFullYear(), date.getMonth(), date.getDate() + days);
}

/** The same day of another month, or that month's last day if it's shorter. */
function addMonths(date: Date, months: number): Date {
  const lastDay = new Date(date.getFullYear(), date.getMonth() + months + 1, 0).getDate();
  return new Date(date.getFullYear(), date.getMonth() + months, Math.min(date.getDate(), lastDay));
}

function isSameDay(left: Date, right: Date): boolean {
  return left.toDateString() === right.toDateString();
}

/** The six weeks shown for `month`, each starting on Monday. */
function weeksOf(month: Date): Date[][] {
  const first = new Date(month.getFullYear(), month.getMonth(), 1);
  const start = addDays(first, -((first.getDay() + 6) % WEEK));
  const weeks: Date[][] = [];
  for (let day = start; weeks.length < 6; day = addDays(day, WEEK)) {
    weeks.push(Array.from({ length: WEEK }, (_, index) => addDays(day, index)));
  }
  return weeks;
}

const KEY_STEPS: Record<string, (day: Date) => Date> = {
  ArrowLeft: (day) => addDays(day, -1),
  ArrowRight: (day) => addDays(day, 1),
  ArrowUp: (day) => addDays(day, -WEEK),
  ArrowDown: (day) => addDays(day, WEEK),
  PageUp: (day) => addMonths(day, -1),
  PageDown: (day) => addMonths(day, 1),
  Home: (day) => addDays(day, -((day.getDay() + 6) % WEEK)),
  End: (day) => addDays(day, 6 - ((day.getDay() + 6) % WEEK)),
};

/**
 * A month of days to pick one from, never before `min`. Arrow keys move by
 * day and week, Page Up and Page Down by month, Home and End to the week's
 * ends; the month slides the way you moved.
 */
export function Calendar({ value, min, onChange }: { value: Date; min: Date; onChange: (day: Date) => void }) {
  const earliest = startOfDay(min);
  const [shown, setShown] = useState(() => new Date(value.getFullYear(), value.getMonth(), 1));
  const [direction, setDirection] = useState(0);
  const grid = useRef<HTMLDivElement>(null);
  const shouldFocusValue = useRef(false);

  const showMonthOf = (day: Date) => {
    const month = new Date(day.getFullYear(), day.getMonth(), 1);
    if (month.getTime() === shown.getTime()) return;
    setDirection(month > shown ? 1 : -1);
    setShown(month);
  };
  // A pick from elsewhere (like a typed time) brings its month into view.
  useEffect(() => {
    const month = new Date(value.getFullYear(), value.getMonth(), 1);
    setShown((current) => {
      if (month.getTime() === current.getTime()) return current;
      setDirection(month > current ? 1 : -1);
      return month;
    });
  }, [value]);
  // After a key moves the choice, focus follows it, even into a new month.
  useEffect(() => {
    if (!shouldFocusValue.current) return;
    shouldFocusValue.current = false;
    grid.current?.querySelector<HTMLButtonElement>('button[aria-pressed="true"]')?.focus();
  });

  const pick = (day: Date) => {
    if (day < earliest) return;
    showMonthOf(day);
    onChange(day);
  };
  const onKeyDown = (event: KeyboardEvent) => {
    const step = KEY_STEPS[event.key];
    if (!step) return;
    event.preventDefault();
    const next = step(startOfDay(value));
    if (next < earliest) return;
    shouldFocusValue.current = true;
    pick(next);
  };
  const monthLabel = shown.toLocaleDateString([], { month: "long", year: "numeric" });
  const isAtEarliestMonth = shown <= new Date(earliest.getFullYear(), earliest.getMonth(), 1);

  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-center gap-1">
        <span aria-live="polite" className="flex-1 pl-1 text-[13px] font-medium">
          {monthLabel}
        </span>
        <IconButton
          label="Previous month"
          size="sm"
          icon={<ChevronLeft size={14} />}
          disabled={isAtEarliestMonth}
          onClick={() => showMonthOf(addMonths(shown, -1))}
        />
        <IconButton
          label="Next month"
          size="sm"
          icon={<ChevronRight size={14} />}
          onClick={() => showMonthOf(addMonths(shown, 1))}
        />
      </div>
      <div className="relative overflow-hidden">
        <AnimatePresence initial={false} mode="popLayout" custom={direction}>
          <MonthGrid
            key={shown.getTime()}
            month={shown}
            value={value}
            earliest={earliest}
            direction={direction}
            gridRef={grid}
            onKeyDown={onKeyDown}
            onPick={pick}
          />
        </AnimatePresence>
      </div>
    </div>
  );
}

/** One month's days; the month sliding out is inert while it goes. */
function MonthGrid({
  month,
  value,
  earliest,
  direction,
  gridRef,
  onKeyDown,
  onPick,
}: {
  month: Date;
  value: Date;
  earliest: Date;
  direction: number;
  gridRef: RefObject<HTMLDivElement | null>;
  onKeyDown: (event: KeyboardEvent) => void;
  onPick: (day: Date) => void;
}) {
  const isPresent = useIsPresent();
  const label = month.toLocaleDateString([], { month: "long", year: "numeric" });
  const weekdayNames = weeksOf(month)[0].map((day) => ({
    short: day.toLocaleDateString([], { weekday: "narrow" }),
    long: day.toLocaleDateString([], { weekday: "long" }),
  }));
  return (
    <motion.div
      ref={gridRef}
      onKeyDown={onKeyDown}
      // The month sliding out stays a moment; nothing in it can be reached meanwhile.
      inert={!isPresent}
      aria-hidden={!isPresent}
      custom={direction}
      initial={{ opacity: 0, x: direction * 24 }}
      animate={{ opacity: 1, x: 0 }}
      exit={{ opacity: 0, x: direction * -24 }}
      transition={springs.snappy}
    >
      <table aria-label={label} className="w-full table-fixed border-collapse">
        <thead>
          <tr>
            {weekdayNames.map((name) => (
              <th
                key={name.long}
                scope="col"
                abbr={name.long}
                className="h-6 text-center text-[11px] font-normal text-faint"
              >
                {name.short}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {weeksOf(month).map((week) => (
            <tr key={week[0].getTime()}>
              {week.map((day) => {
                const isSelected = isSameDay(day, value);
                const isPast = day < earliest;
                // Days of the months around are left blank, so each date appears once.
                if (day.getMonth() !== month.getMonth()) return <td key={day.getTime()} />;
                return (
                  <td key={day.getTime()} className="p-0">
                    <button
                      type="button"
                      tabIndex={isSelected ? 0 : -1}
                      aria-pressed={isSelected}
                      disabled={isPast}
                      aria-label={day.toLocaleDateString([], { weekday: "long", month: "long", day: "numeric" })}
                      aria-current={isSameDay(day, earliest) ? "date" : undefined}
                      onClick={() => onPick(day)}
                      className={cx(
                        "relative grid h-8 w-full place-items-center rounded-lg text-[12.5px] tabular-nums transition-colors",
                        isSelected
                          ? "font-semibold text-white"
                          : "text-ink hover:bg-surface-2 disabled:text-decorative disabled:hover:bg-transparent",
                      )}
                    >
                      {isSelected && (
                        <motion.span
                          layoutId="calendar-selected"
                          transition={springs.snappy}
                          className="absolute inset-0.5 rounded-lg bg-accent"
                        />
                      )}
                      <span className="relative">{day.getDate()}</span>
                    </button>
                  </td>
                );
              })}
            </tr>
          ))}
        </tbody>
      </table>
    </motion.div>
  );
}
