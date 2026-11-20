import { AtSign, MessageCircle, Users } from "lucide-react";
import { motion } from "motion/react";
import { Popover as RadixPopover } from "radix-ui";
import { createContext, type ReactNode, type RefObject, useContext, useEffect, useMemo, useRef, useState } from "react";
import { api, type Member } from "../../lib/api";
import { useMember } from "../../lib/members";
import { displayName, formatStatusEnd, presenceLabel } from "../../lib/people";
import { reportError } from "../../lib/toast";
import { useWorkspace } from "../../lib/workspace";
import { Avatar, Badge, Button, pop, springs } from "../../ui";
import { alertsLabel, shownStatus } from "./StatusEmoji";

/** WCAG 1.4.13: a hover card waits for intent, and lingers so the pointer can move onto it. */
const HOVER_OPEN_MS = 600;
const HOVER_CLOSE_MS = 300;

/** "pointer": a click; "keyboard": Enter or Space on the name. */
type OpenedBy = "hover" | "pointer" | "keyboard";

type HoverProps = {
  onPointerEnter: (event: React.PointerEvent) => void;
  onPointerLeave: () => void;
};

type CardControl = {
  name: string;
  hoverProps: HoverProps;
  /** Opens by click or keyboard (focus moves in), or closes a card opened that way. */
  toggleByClick: (event: React.MouseEvent) => void;
  extraTarget: RefObject<HTMLElement | null>;
};

const CardContext = createContext<CardControl | null>(null);

export const CARD_SURFACE = "z-50 w-[280px] rounded-xl border border-line bg-surface p-4 shadow-float outline-none";

/**
 * A member's profile card: who they are, their status and presence, and
 * quick actions. Wraps the names and avatars that open it
 * (`ProfileCardTrigger`, `ProfileCardAvatar`). Clicking or Enter opens it
 * with focus inside, and Esc returns focus; triggers marked `opensOnHover`
 * also open it after a pause under the pointer, without taking focus.
 */
export function ProfileCard({
  memberId,
  onMention,
  children,
}: {
  memberId: string;
  /** Mentions them in the message box nearby; without it, the card offers no Mention. */
  onMention?: (name: string, memberId: string) => void;
  children: ReactNode;
}) {
  const member = useMember(memberId);
  const { openSettings, navigate } = useWorkspace();
  const [isOpen, setOpen] = useState(false);
  const openedBy = useRef<OpenedBy>("pointer");
  const isFocusHandedOn = useRef(false);
  const timers = useRef<{ open?: ReturnType<typeof setTimeout>; close?: ReturnType<typeof setTimeout> }>({});
  const card = useRef<HTMLDivElement>(null);
  const extraTarget = useRef<HTMLElement>(null);
  /** The name or avatar that opened the card, for focus to go back to. */
  const opener = useRef<HTMLElement | null>(null);
  const isOpenRef = useRef(isOpen);
  isOpenRef.current = isOpen;

  useEffect(() => {
    const pending = timers.current;
    return () => {
      clearTimeout(pending.open);
      clearTimeout(pending.close);
    };
  }, []);

  const control = useMemo((): CardControl | null => {
    if (!member) return null;
    const clearTimers = () => {
      clearTimeout(timers.current.open);
      clearTimeout(timers.current.close);
    };
    const scheduleClose = () => {
      clearTimeout(timers.current.close);
      timers.current.close = setTimeout(() => setOpen(false), HOVER_CLOSE_MS);
    };
    return {
      name: displayName(member.name, member.id),
      extraTarget,
      hoverProps: {
        onPointerEnter: (event) => {
          if (event.pointerType !== "mouse") return;
          clearTimeout(timers.current.close);
          if (isOpenRef.current) return;
          const target = event.currentTarget;
          timers.current.open = setTimeout(() => {
            opener.current = target instanceof HTMLElement ? target : null;
            openedBy.current = "hover";
            setOpen(true);
          }, HOVER_OPEN_MS);
        },
        onPointerLeave: () => {
          clearTimeout(timers.current.open);
          if (isOpenRef.current && openedBy.current === "hover") scheduleClose();
        },
      },
      toggleByClick: (event) => {
        // The card decides; Radix's own toggle would close a card the pointer just opened.
        event.preventDefault();
        clearTimers();
        opener.current = event.currentTarget instanceof HTMLElement ? event.currentTarget : null;
        if (isOpenRef.current && openedBy.current !== "hover") return setOpen(false);
        const wasHoverOpen = isOpenRef.current;
        // A keyboard press on a button clicks with detail 0.
        openedBy.current = event.detail === 0 ? "keyboard" : "pointer";
        setOpen(true);
        if (wasHoverOpen) focusInside(card.current, openedBy.current);
      },
    };
  }, [member]);

  const close = () => {
    clearTimeout(timers.current.close);
    setOpen(false);
  };
  /** Closes the card with focus moved on by the caller, not back to the name. */
  const closeHandingFocusOn = () => {
    isFocusHandedOn.current = true;
    close();
  };
  // The same tree with or without the member, so rows don't remount when members load.
  return (
    <RadixPopover.Root open={isOpen && control !== null} onOpenChange={(open) => !open && close()}>
      <CardContext.Provider value={control}>{children}</CardContext.Provider>
      {member && control && (
        <RadixPopover.Portal>
          <RadixPopover.Content
            side="bottom"
            align="start"
            sideOffset={6}
            collisionPadding={12}
            aria-label={`${control.name}'s profile`}
            asChild
            onOpenAutoFocus={(event) => {
              if (openedBy.current === "keyboard") return;
              event.preventDefault();
              if (openedBy.current === "pointer") focusInside(card.current, "pointer");
            }}
            onCloseAutoFocus={(event) => {
              // Radix skips its own return after a click on the name (that counts as outside), so it's done here.
              event.preventDefault();
              if (openedBy.current !== "hover" && !isFocusHandedOn.current) opener.current?.focus();
              isFocusHandedOn.current = false;
            }}
            onInteractOutside={(event) => {
              if (event.target instanceof Node && extraTarget.current?.contains(event.target)) event.preventDefault();
            }}
            onPointerEnter={() => clearTimeout(timers.current.close)}
            onPointerLeave={() => openedBy.current === "hover" && control.hoverProps.onPointerLeave()}
          >
            <motion.div ref={card} tabIndex={-1} {...pop} transition={springs.snappy} className={CARD_SURFACE}>
              <CardBody
                member={member}
                onMention={
                  onMention &&
                  (() => {
                    closeHandingFocusOn();
                    onMention(member.name, member.id);
                  })
                }
                onMessage={() => {
                  closeHandingFocusOn();
                  api.openDm([member.id]).then((id) => navigate({ kind: "channel", id }), reportError);
                }}
                onViewInMembers={() => {
                  closeHandingFocusOn();
                  openSettings({ section: "members", membersTab: "members" });
                }}
              />
            </motion.div>
          </RadixPopover.Content>
        </RadixPopover.Portal>
      )}
    </RadixPopover.Root>
  );
}

/** A name or @mention that opens the card. Outside a `ProfileCard` (an unknown member), it's plain text. */
export function ProfileCardTrigger({
  children,
  className,
  opensOnHover = false,
}: {
  children: ReactNode;
  className?: string;
  /** Names in message headers: pausing on them opens the card too. */
  opensOnHover?: boolean;
}) {
  const control = useContext(CardContext);
  if (!control) return <span className={className}>{children}</span>;
  return (
    <RadixPopover.Trigger asChild>
      <button
        type="button"
        className={className}
        onClick={control.toggleByClick}
        {...(opensOnHover ? control.hoverProps : {})}
      >
        {children}
      </button>
    </RadixPopover.Trigger>
  );
}

/**
 * An avatar that opens the card on click or a pause under the pointer. The
 * name beside it is the keyboard's way in, so this isn't a tab stop.
 */
export function ProfileCardAvatar({ children }: { children: ReactNode }) {
  const control = useContext(CardContext);
  if (!control) return children;
  return (
    <button
      ref={(element) => {
        control.extraTarget.current = element;
      }}
      type="button"
      tabIndex={-1}
      aria-label={`Show ${control.name}'s profile`}
      className="block rounded-[30%]"
      onClick={control.toggleByClick}
      {...control.hoverProps}
    >
      {children}
    </button>
  );
}

export function CardBody({
  member,
  onMention,
  onMessage,
  onViewInMembers,
}: {
  member: Member;
  /** Absent where there's no message box to mention them in. */
  onMention: (() => void) | undefined;
  /** Opens your DM with them. */
  onMessage: () => void;
  onViewInMembers: () => void;
}) {
  const { session } = useWorkspace();
  const name = displayName(member.name, member.id);
  const isMe = member.id === session.me;
  const status = shownStatus(member);
  const alerts = status && alertsLabel(status);
  const canMention = member.active && !isMe && member.name !== "";

  return (
    <div className="flex flex-col gap-3">
      <div className="flex items-center gap-3">
        <Avatar
          id={member.id}
          name={name}
          isBot={member.is_bot}
          size={48}
          presence={member.active && !member.is_bot ? member.presence : undefined}
        />
        <div className="min-w-0 flex-1">
          <div className="flex items-baseline gap-1.5">
            <h2 className="truncate text-[15px] font-semibold">{name}</h2>
            {isMe && <span className="shrink-0 text-[12px] text-muted">(you)</span>}
          </div>
          <div className="mt-0.5 flex flex-wrap items-center gap-1.5 text-[12px] text-muted">
            {member.is_admin && <Badge>Admin</Badge>}
            {member.is_bot && <Badge tone="accent">Agent</Badge>}
            {!member.active && <span>No longer in this workspace</span>}
            {member.active && !member.is_bot && <span>{presenceLabel(member.presence)}</span>}
          </div>
        </div>
      </div>
      {status && (
        <div className="rounded-lg bg-surface-2 px-3 py-2 text-[13px]">
          {(status.emoji || status.text) && (
            <p className="selectable">{[status.emoji, status.text].filter(Boolean).join(" ")}</p>
          )}
          {(status.until_ms !== null || alerts) && (
            <p className="mt-0.5 text-[12px] text-muted">
              {status.until_ms !== null && `Until ${formatStatusEnd(status.until_ms)}`}
              {status.until_ms !== null && alerts && " · "}
              {alerts && <span className="text-warning-text">{alerts}</span>}
            </p>
          )}
        </div>
      )}
      <div className="flex flex-wrap gap-2">
        {member.active && !isMe && (
          <Button size="sm" variant="primary" icon={<MessageCircle size={13} />} onClick={onMessage}>
            Message
          </Button>
        )}
        {canMention && onMention && (
          <Button size="sm" icon={<AtSign size={13} />} onClick={onMention}>
            Mention
          </Button>
        )}
        <Button size="sm" variant="ghost" icon={<Users size={13} />} onClick={onViewInMembers}>
          View in members
        </Button>
      </div>
    </div>
  );
}

/**
 * Where focus goes as the card opens: its first action after a keyboard
 * press, the card itself after a click. A click must show no focus ring,
 * and WebKit can count a button focused by script as :focus-visible.
 */
export function focusInside(card: HTMLElement | null, openedBy: "pointer" | "keyboard") {
  if (openedBy === "keyboard") card?.querySelector<HTMLElement>("button")?.focus();
  else card?.focus();
}
