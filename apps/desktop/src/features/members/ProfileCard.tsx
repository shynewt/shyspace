import { motion } from "motion/react";
import { Popover as RadixPopover } from "radix-ui";
import { createContext, type ReactNode, type RefObject, useContext, useEffect, useMemo, useRef, useState } from "react";
import type { Member } from "../../lib/api";
import { useMember } from "../../lib/members";
import { displayName } from "../../lib/people";
import { useWorkspace } from "../../lib/workspace";
import { pop } from "../../ui";
import { ProfileActions } from "./ProfileActions";
import { Bio, DetailRows, ProfileHeader, StatusBox } from "./ProfileParts";

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

export const CARD_SURFACE =
  "z-50 w-[320px] origin-(--radix-popover-content-transform-origin) rounded-xl border border-line bg-surface p-4 shadow-float outline-none";

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
  const { openProfile } = useWorkspace();
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
            onPointerDownCapture={() => {
              // Using a card that opened on hover keeps it open, like one opened by a click.
              if (openedBy.current !== "hover") return;
              clearTimeout(timers.current.close);
              openedBy.current = "pointer";
            }}
            onPointerLeave={() => openedBy.current === "hover" && control.hoverProps.onPointerLeave()}
          >
            <motion.div ref={card} tabIndex={-1} {...pop} className={CARD_SURFACE}>
              <CardBody
                member={member}
                onLeave={closeHandingFocusOn}
                onMention={
                  onMention &&
                  (() => {
                    closeHandingFocusOn();
                    onMention(member.name, member.id);
                  })
                }
                onViewFullProfile={() => {
                  closeHandingFocusOn();
                  openProfile(member.id, opener.current);
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
  onLeave,
  onMention,
  onViewFullProfile,
}: {
  member: Member;
  /** Closes the card as an action takes you elsewhere, focus moving on with it. */
  onLeave: () => void;
  /** Absent where there's no message box to mention them in. */
  onMention: (() => void) | undefined;
  onViewFullProfile: () => void;
}) {
  const { session } = useWorkspace();
  const [isBioCut, setBioCut] = useState(false);
  return (
    <div className="flex flex-col gap-3">
      <ProfileHeader member={member} isMe={member.id === session.me} />
      <StatusBox member={member} />
      <DetailRows member={member} />
      {member.details.bio && (
        <div className="flex flex-col items-start gap-1">
          <Bio text={member.details.bio} className="line-clamp-3" onCut={setBioCut} />
          {isBioCut && (
            <button
              type="button"
              onClick={onViewFullProfile}
              className="rounded text-sm font-medium text-accent-text hover:underline"
            >
              View full profile
            </button>
          )}
        </div>
      )}
      <div className="border-t border-line pt-3">
        <ProfileActions member={member} onLeave={onLeave} onMention={onMention} onViewFullProfile={onViewFullProfile} />
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
