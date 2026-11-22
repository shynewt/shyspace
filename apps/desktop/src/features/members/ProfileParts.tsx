import { CalendarClock, Clock, Globe, Mail, MapPin, Phone } from "lucide-react";
import { type ReactNode, useLayoutEffect, useRef } from "react";
import type { Member, WorkingHours } from "../../lib/api";
import { hasUserInfo, mailtoUrl, openExternal, shownWebsite, telUrl } from "../../lib/contact";
import {
  describeTimeDifference,
  describeWorkingHours,
  deviceTimeZone,
  formatTimeIn,
  isKnownTimeZone,
  isWithinWorkingHours,
  wallClockIn,
} from "../../lib/localTime";
import { displayName, formatStatusEnd, presenceLabel } from "../../lib/people";
import { useMinuteClock } from "../../lib/useMinuteClock";
import { Avatar, Badge, CopyButton, cx, Tooltip } from "../../ui";
import { alertsLabel, shownStatus } from "./StatusEmoji";

const ICON_SIZE = 14;
/** Plain web addresses in a bio, up to the next space. */
const WEB_LINK = /https?:\/\/[^\s]+/gi;
/** Punctuation that ends a sentence rather than an address. */
const TRAILING_PUNCTUATION = /[.,;:!?)\]'"]+$/;

/** Who they are: avatar with presence, name and pronouns, title, tags. */
export function ProfileHeader({ member, isMe }: { member: Member; isMe: boolean }) {
  const name = displayName(member.name, member.id);
  const { title, pronouns } = member.details;
  return (
    <div className="flex items-center gap-3">
      <Avatar
        id={member.id}
        name={name}
        isBot={member.is_bot}
        size={64}
        presence={member.active && !member.is_bot ? member.presence : undefined}
      />
      <div className="flex min-w-0 flex-1 flex-col gap-0.5">
        <div className="flex min-w-0 items-baseline gap-1.5 leading-5">
          <h2 className="selectable truncate text-lg font-semibold">{name}</h2>
          {pronouns && <span className="selectable max-w-[45%] shrink-0 truncate text-sm text-muted">{pronouns}</span>}
          {isMe && <span className="shrink-0 text-xs text-muted">(you)</span>}
        </div>
        {title && <p className="selectable truncate text-sm leading-[18px] text-muted">{title}</p>}
        <div className="flex min-w-0 items-center gap-1.5 overflow-hidden text-xs leading-[18px] whitespace-nowrap text-muted">
          {member.is_admin && <Badge>Admin</Badge>}
          {member.is_bot && <Badge tone="accent">Agent</Badge>}
          {!member.active && <span>No longer in this workspace</span>}
          {member.active && !member.is_bot && <span>{presenceLabel(member.presence)}</span>}
        </div>
      </div>
    </div>
  );
}

/** Their status with when it ends and how quiet their alerts are; nothing for agents or without one. */
export function StatusBox({ member }: { member: Member }) {
  const status = shownStatus(member);
  if (!status) return null;
  const alerts = alertsLabel(status);
  return (
    <div className="rounded-lg bg-surface-2 px-3 py-2 text-sm">
      {(status.emoji || status.text) && (
        <p className="selectable">{[status.emoji, status.text].filter(Boolean).join(" ")}</p>
      )}
      {(status.until_ms !== null || alerts) && (
        <p className="mt-0.5 text-xs text-muted">
          {status.until_ms !== null && `Until ${formatStatusEnd(status.until_ms)}`}
          {status.until_ms !== null && alerts && " · "}
          {alerts && <span className="text-warning-text">{alerts}</span>}
        </p>
      )}
    </div>
  );
}

function DetailRow({
  icon,
  label,
  children,
  action,
}: {
  icon: ReactNode;
  label: string;
  children: ReactNode;
  action?: ReactNode;
}) {
  return (
    <li className="flex min-h-7 items-center gap-2.5 text-sm">
      <span className="shrink-0 text-decorative" role="img" aria-label={label}>
        {icon}
      </span>
      <div className="min-w-0 flex-1 truncate">{children}</div>
      {action}
    </li>
  );
}

/** "11:42 PM local time · 8 h ahead · outside working hours", kept current. */
function LocalTimeRow({ zone, hours }: { zone: string; hours: WorkingHours | null }) {
  const now = useMinuteClock();
  const clock = wallClockIn(zone, now);
  const difference = describeTimeDifference(zone, deviceTimeZone(), now) ?? "Same time as you";
  const isOutsideHours = hours !== null && !isWithinWorkingHours(hours, clock);
  return (
    <DetailRow icon={<Clock size={ICON_SIZE} />} label="Local time">
      <p className="truncate">
        <span className="text-ink">{formatTimeIn(zone, now)} local time</span>
        <span className="text-muted"> · {difference}</span>
      </p>
      {isOutsideHours && <p className="truncate text-xs text-warning-text">Outside working hours</p>}
    </DetailRow>
  );
}

function LinkRow({
  icon,
  label,
  url,
  text,
  copyValue,
}: {
  icon: ReactNode;
  label: string;
  url: string;
  text: string;
  copyValue: string;
}) {
  return (
    <DetailRow
      icon={icon}
      label={label}
      action={<CopyButton value={copyValue} label={`Copy ${label.toLowerCase()}`} />}
    >
      <Tooltip label={url}>
        <a
          href={url}
          onClick={(event) => {
            event.preventDefault();
            openExternal(url);
          }}
          className="text-ink underline-offset-2 hover:underline"
        >
          {text}
        </a>
      </Tooltip>
    </DetailRow>
  );
}

/** Their working hours as a detail row, in their time zone. */
export function WorkingHoursRow({ member }: { member: Member }) {
  const { working_hours, time_zone } = member.details;
  if (!working_hours) return null;
  return (
    <DetailRow icon={<CalendarClock size={ICON_SIZE} />} label="Working hours">
      <span className="text-ink">{describeWorkingHours(working_hours)}</span>
      {isKnownTimeZone(time_zone) && <span className="text-muted"> their time</span>}
    </DetailRow>
  );
}

/**
 * The details they filled in, one row each: local time, location, then how
 * to reach them, and `children` (more rows) last.
 */
export function DetailRows({ member, children }: { member: Member; children?: ReactNode }) {
  const { time_zone, working_hours, location, email, phone, website } = member.details;
  const isZoneKnown = isKnownTimeZone(time_zone);
  const rows = [
    isZoneKnown && <LocalTimeRow key="time" zone={time_zone} hours={working_hours} />,
    location && (
      <DetailRow key="location" icon={<MapPin size={ICON_SIZE} />} label="Location">
        <span className="selectable text-ink">{location}</span>
      </DetailRow>
    ),
    email && (
      <LinkRow
        key="email"
        icon={<Mail size={ICON_SIZE} />}
        label="Email address"
        url={mailtoUrl(email)}
        text={email}
        copyValue={email}
      />
    ),
    phone && (
      <LinkRow
        key="phone"
        icon={<Phone size={ICON_SIZE} />}
        label="Phone number"
        url={telUrl(phone)}
        text={phone}
        copyValue={phone}
      />
    ),
    shownWebsite(website) && (
      <LinkRow
        key="website"
        icon={<Globe size={ICON_SIZE} />}
        label="Website"
        url={website}
        text={shownWebsite(website) ?? website}
        copyValue={website}
      />
    ),
  ].filter(Boolean);
  if (rows.length === 0 && !children) return null;
  return (
    <ul aria-label="Details" className="flex flex-col">
      {rows}
      {children}
    </ul>
  );
}

/** Text with its web addresses as links that open in the browser; everything else stays plain. */
function LinkedText({ text }: { text: string }) {
  const parts: ReactNode[] = [];
  let shownUpTo = 0;
  for (const match of text.matchAll(WEB_LINK)) {
    const trailing = match[0].match(TRAILING_PUNCTUATION)?.[0] ?? "";
    const url = match[0].slice(0, match[0].length - trailing.length);
    // An address naming a user before its host reads as one site and opens another: left as text.
    if (hasUserInfo(url)) continue;
    parts.push(text.slice(shownUpTo, match.index));
    parts.push(
      <a
        key={match.index}
        href={url}
        onClick={(event) => {
          event.preventDefault();
          openExternal(url);
        }}
        className="text-accent-text underline-offset-2 hover:underline"
      >
        {url}
      </a>,
    );
    shownUpTo = match.index + url.length;
  }
  parts.push(text.slice(shownUpTo));
  return parts;
}

/** Their bio as they wrote it, lines kept. `onCut` tells whether a `line-clamp` in `className` cut it short. */
export function Bio({
  text,
  className,
  onCut,
}: {
  text: string;
  className?: string;
  onCut?: (isCut: boolean) => void;
}) {
  const paragraph = useRef<HTMLParagraphElement>(null);
  useLayoutEffect(() => {
    const element = paragraph.current;
    if (element && onCut) onCut(element.scrollHeight > element.clientHeight + 1);
  }, [onCut]);
  return (
    <p
      ref={paragraph}
      className={cx("selectable text-sm leading-relaxed break-words whitespace-pre-line text-ink", className)}
    >
      <LinkedText text={text} />
    </p>
  );
}
