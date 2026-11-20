import { AtSign, Users } from "lucide-react";
import { motion } from "motion/react";
import { Popover as RadixPopover } from "radix-ui";
import { createContext, type ReactNode, type RefObject, useContext, useEffect, useMemo, useRef, useState } from "react";
import type { Member } from "../../lib/api";
import { useMember } from "../../lib/members";
import { displayName, formatStatusEnd, presenceLabel } from "../../lib/people";
import { useWorkspace } from "../../lib/workspace";
import { Avatar, Badge, Button, pop, springs } from "../../ui";
import { alertsLabel, shownStatus } from "./StatusEmoji";

/** WCAG 1.4.13: a hover card waits for intent, and lingers so the pointer can move onto it. */
const HOVER_OPEN_MS = 600;
const HOVER_CLOSE_MS = 300;

type OpenedBy = "hover" | "click";

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
  const [isOpen, setOpen] = useState(false);
  const openedBy = useRef<OpenedBy>("click");
  const isFocusHandedOn = useRef(false);
  const timers = useRef<{ open?: ReturnType<typeof setTimeout>; close?: ReturnType<typeof setTimeout> }>({});
  const card = useRef<HTMLDivElement>(null);
  const extraTarget = useRef<HTMLElement>(null);
  const isOpenRef = useRef(isOpen);
  isOpenRef.current = isOpen;

  useEffect(() => {
    const pending = timers.current;
    return () => {
      clearTimeout(pending.open);
      clearTimeout(pending.close);
    };
  }, []);

  const control = useMemo((): CardControl => {
    const clearTimers = () => {
      clearTimeout(timers.current.open);
      clearTimeout(timers.current.close);
    };
    const scheduleClose = () => {
      clearTimeout(timers.current.close);
      timers.current.close = setTimeout(() => setOpen(false), HOVER_CLOSE_MS);
    };
    return {
      name: member ? displayName(member.name, member.id) : "",
      extraTarget,
      hoverProps: {
        onPointerEnter: (event) => {
          if (event.pointerType !== "mouse") return;
          clearTimeout(timers.current.close);
          if (isOpenRef.current) return;
          timers.current.open = setTimeout(() => {
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
        if (isOpenRef.current && openedBy.current === "click") return setOpen(false);
        const wasHoverOpen = isOpenRef.current;
        openedBy.current = "click";
        setOpen(true);
        if (wasHoverOpen) card.current?.querySelector<HTMLElement>("button")?.focus();
      },
    };
  }, [member]);

  if (!member) return children;
  const close = () => {
    clearTimeout(timers.current.close);
    setOpen(false);
  };
  const hoverOnly = openedBy.current === "hover";
  return (
    <RadixPopover.Root open={isOpen} onOpenChange={(open) => !open && close()}>
      <CardContext.Provider value={control}>{children}</CardContext.Provider>
      <RadixPopover.Portal>
        <RadixPopover.Content
          side="bottom"
          align="start"
          sideOffset={6}
          collisionPadding={12}
          aria-label={`${control.name}'s profile`}
          asChild
          onOpenAutoFocus={(event) => hoverOnly && event.preventDefault()}
          onCloseAutoFocus={(event) => {
            if (hoverOnly || isFocusHandedOn.current) event.preventDefault();
            isFocusHandedOn.current = false;
          }}
          onInteractOutside={(event) => {
            if (event.target instanceof Node && extraTarget.current?.contains(event.target)) event.preventDefault();
          }}
          onPointerEnter={() => clearTimeout(timers.current.close)}
          onPointerLeave={() => openedBy.current === "hover" && control.hoverProps.onPointerLeave()}
        >
          <motion.div
            ref={card}
            {...pop}
            transition={springs.snappy}
            className="z-50 w-[280px] rounded-xl border border-line bg-surface p-4 shadow-float outline-none"
          >
            <CardBody
              member={member}
              onMention={onMention}
              onClose={close}
              onFocusHandedOn={() => {
                isFocusHandedOn.current = true;
              }}
            />
          </motion.div>
        </RadixPopover.Content>
      </RadixPopover.Portal>
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

function CardBody({
  member,
  onMention,
  onClose,
  onFocusHandedOn,
}: {
  member: Member;
  onMention: ((name: string, memberId: string) => void) | undefined;
  onClose: () => void;
  onFocusHandedOn: () => void;
}) {
  const { session, openSettings } = useWorkspace();
  const name = displayName(member.name, member.id);
  const isMe = member.id === session.me;
  const status = shownStatus(member);
  const alerts = status && alertsLabel(status);
  const canMention = onMention && member.active && !isMe && member.name !== "";

  const mention = () => {
    onFocusHandedOn();
    onClose();
    onMention?.(member.name, member.id);
  };
  const viewInMembers = () => {
    onFocusHandedOn();
    onClose();
    openSettings({ section: "members", membersTab: "members" });
  };

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
      <div className="flex gap-2">
        {canMention && (
          <Button size="sm" icon={<AtSign size={13} />} onClick={mention}>
            Mention
          </Button>
        )}
        <Button size="sm" variant="ghost" icon={<Users size={13} />} onClick={viewInMembers}>
          View in members
        </Button>
      </div>
    </div>
  );
}
