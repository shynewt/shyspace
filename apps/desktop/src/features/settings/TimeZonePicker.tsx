import { Check, ChevronDown, Globe, Monitor, Search, X } from "lucide-react";
import { motion } from "motion/react";
import { type ReactNode, useEffect, useId, useMemo, useRef, useState } from "react";
import {
  cityOf,
  deviceTimeZone,
  formatOffset,
  isKnownTimeZone,
  offsetMinutes,
  timeZoneNames,
} from "../../lib/localTime";
import { cx, type FieldDescription, instantAfterAKey, Popover, springs } from "../../ui";

type Zone = { name: string; city: string; offset: string; offsetMinutes: number; searchText: string };

type Choice = { key: string; label: ReactNode; name: string; detail?: string; icon?: ReactNode; zone: string };

/** "Lisbon · GMT+0", or the bare name for a zone this device doesn't know. */
export function describeZone(zone: string, at = new Date()): string {
  if (!isKnownTimeZone(zone)) return zone;
  return `${cityOf(zone)} · ${formatOffset(offsetMinutes(zone, at))}`;
}

function listZones(at: Date): Zone[] {
  return timeZoneNames()
    .map((name) => {
      const minutes = offsetMinutes(name, at);
      const offset = formatOffset(minutes);
      return {
        name,
        city: cityOf(name),
        offset,
        offsetMinutes: minutes,
        searchText: `${name.replaceAll("_", " ")} ${cityOf(name)} ${offset}`.toLowerCase(),
      };
    })
    .sort((left, right) => left.offsetMinutes - right.offsetMinutes || left.city.localeCompare(right.city));
}

/**
 * Picks the time zone to share: a button showing the choice, opening a
 * searchable list with this computer's zone first as a suggestion and a way
 * to stop sharing it.
 */
export function TimeZonePicker({
  id,
  fieldName,
  value,
  onChange,
  description,
}: {
  id: string;
  /** Marks the button as this form field, for focusing it from outside. */
  fieldName: string;
  /** An IANA name, or "" for not shared. */
  value: string;
  onChange: (zone: string) => void;
  description: FieldDescription;
}) {
  const [isOpen, setOpen] = useState(false);
  return (
    <Popover
      open={isOpen}
      onOpenChange={setOpen}
      side="bottom"
      align="start"
      trigger={
        <button
          id={id}
          type="button"
          data-field={fieldName}
          {...description}
          className="flex h-9 w-full items-center gap-2 rounded-lg border border-line bg-bg px-3 text-left text-sm transition hover:border-surface-3 data-[state=open]:border-accent"
        >
          <Globe size={14} className="shrink-0 text-decorative" />
          <span className={cx("min-w-0 flex-1 truncate", !value && "text-faint")}>
            {value ? describeZone(value) : "Not shared"}
          </span>
          <ChevronDown size={14} className="shrink-0 text-decorative" />
        </button>
      }
    >
      <ZoneList
        value={value}
        onChoose={(zone) => {
          onChange(zone);
          setOpen(false);
        }}
      />
    </Popover>
  );
}

function ZoneList({ value, onChoose }: { value: string; onChoose: (zone: string) => void }) {
  const zones = useMemo(() => listZones(new Date()), []);
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const listId = useId();
  const list = useRef<HTMLDivElement>(null);
  const own = deviceTimeZone();

  const words = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
  const matches = zones.filter((zone) => words.every((word) => zone.searchText.includes(word)));
  const suggestions: Choice[] = query.trim()
    ? []
    : [
        ...(own !== value
          ? [
              {
                key: "own",
                label: (
                  <>
                    Use this computer's time zone <span className="text-muted">({own})</span>
                  </>
                ),
                name: `Use this computer's time zone (${own})`,
                icon: <Monitor size={14} />,
                zone: own,
              },
            ]
          : []),
        ...(value ? [{ key: "none", label: "Don't share", name: "Don't share", icon: <X size={14} />, zone: "" }] : []),
      ];
  const choices: Choice[] = [
    ...suggestions,
    ...matches.map((zone) => ({
      key: zone.name,
      label: zone.city,
      name: `${zone.city} · ${zone.offset} (${zone.name})`,
      detail: zone.offset,
      zone: zone.name,
    })),
  ];
  const activeIndex = Math.min(active, choices.length - 1);

  useEffect(() => {
    list.current?.querySelector(`[data-index="${activeIndex}"]`)?.scrollIntoView({ block: "nearest" });
  }, [activeIndex]);

  return (
    <div className="flex w-popover flex-col">
      <div className="relative p-1">
        <Search size={14} className="pointer-events-none absolute top-1/2 left-3.5 -translate-y-1/2 text-decorative" />
        <input
          value={query}
          role="combobox"
          aria-label="Search time zones"
          aria-expanded
          aria-controls={listId}
          aria-activedescendant={choices.length > 0 ? `${listId}-${activeIndex}` : undefined}
          placeholder="Search by city or GMT offset"
          onChange={(event) => {
            setQuery(event.target.value);
            setActive(0);
          }}
          onKeyDown={(event) => {
            if (event.key === "ArrowDown") {
              event.preventDefault();
              setActive(Math.min(activeIndex + 1, choices.length - 1));
            } else if (event.key === "ArrowUp") {
              event.preventDefault();
              setActive(Math.max(activeIndex - 1, 0));
            } else if (event.key === "Enter") {
              event.preventDefault();
              const chosen = choices[activeIndex];
              if (chosen) onChoose(chosen.zone);
            }
          }}
          className="h-9 w-full rounded-lg bg-transparent pr-3 pl-8 text-sm outline-none placeholder:text-faint"
        />
      </div>
      <div
        ref={list}
        id={listId}
        role="listbox"
        aria-label="Time zones"
        className="max-h-64 overflow-y-auto border-t border-line p-1"
      >
        {choices.length === 0 && (
          <p className="px-3 py-4 text-center text-sm text-muted">No time zone matches “{query.trim()}”.</p>
        )}
        {choices.map((choice, index) => (
          <ZoneRow
            key={choice.key}
            id={`${listId}-${index}`}
            index={index}
            choice={choice}
            isActive={index === activeIndex}
            isChosen={choice.zone === value && choice.key !== "own"}
            hasDividerAfter={index === suggestions.length - 1}
            onHover={() => setActive(index)}
            onChoose={() => onChoose(choice.zone)}
          />
        ))}
      </div>
    </div>
  );
}

function ZoneRow({
  id,
  index,
  choice,
  isActive,
  isChosen,
  hasDividerAfter,
  onHover,
  onChoose,
}: {
  id: string;
  index: number;
  choice: Choice;
  isActive: boolean;
  isChosen: boolean;
  hasDividerAfter: boolean;
  onHover: () => void;
  onChoose: () => void;
}) {
  return (
    <>
      <button
        id={id}
        type="button"
        data-index={index}
        role="option"
        aria-selected={isActive}
        aria-label={choice.name}
        tabIndex={-1}
        onMouseMove={onHover}
        onClick={onChoose}
        className="relative flex h-8 w-full cursor-default items-center gap-2 rounded-lg px-2.5 text-left text-sm"
      >
        {isActive && (
          <motion.span
            layoutId="time-zone-option"
            transition={instantAfterAKey(springs.snappy)}
            className="absolute inset-0 rounded-lg bg-surface-2"
          />
        )}
        {choice.icon && <span className="relative text-decorative">{choice.icon}</span>}
        <span className="relative min-w-0 flex-1 truncate">{choice.label}</span>
        {choice.detail && <span className="relative shrink-0 text-xs text-muted tabular-nums">{choice.detail}</span>}
        <Check size={13} className={cx("relative shrink-0 text-accent", !isChosen && "invisible")} />
      </button>
      {hasDividerAfter && <div aria-hidden className="mx-2 my-1 h-px bg-line" />}
    </>
  );
}
