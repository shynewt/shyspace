import { motion } from "motion/react";
import { Popover as RadixPopover } from "radix-ui";
import { useRef } from "react";
import { api } from "../../lib/api";
import { useMember } from "../../lib/members";
import { displayName } from "../../lib/people";
import { reportError } from "../../lib/toast";
import { useWorkspace } from "../../lib/workspace";
import { pop, springs } from "../../ui";
import { CARD_SURFACE, CardBody, focusInside } from "./ProfileCard";

/** Something the card can be placed against: an element, or a spot like the cursor's. */
type Anchor = { getBoundingClientRect: () => DOMRect };

/**
 * A profile card opened from outside React's tree, like a mention chip in
 * the page editor, placed against `anchor`. Esc or a click outside calls
 * `onDismiss` (go back to where you were); an action calls `onLeave`
 * (focus moves on with it). An unknown member shows nothing.
 */
export function ProfileCardAt({
  memberId,
  anchor,
  openedBy,
  onDismiss,
  onLeave,
}: {
  memberId: string;
  anchor: Anchor;
  openedBy: "pointer" | "keyboard";
  onDismiss: () => void;
  onLeave: () => void;
}) {
  const member = useMember(memberId);
  const { openSettings, navigate } = useWorkspace();
  const card = useRef<HTMLDivElement>(null);
  if (!member) return null;
  return (
    <RadixPopover.Root open onOpenChange={(open) => !open && onDismiss()}>
      <RadixPopover.Anchor virtualRef={{ current: anchor }} />
      <RadixPopover.Portal>
        <RadixPopover.Content
          side="bottom"
          align="start"
          sideOffset={6}
          collisionPadding={12}
          aria-label={`${displayName(member.name, member.id)}'s profile`}
          asChild
          onOpenAutoFocus={(event) => {
            event.preventDefault();
            focusInside(card.current, openedBy);
          }}
          onCloseAutoFocus={(event) => event.preventDefault()}
        >
          <motion.div ref={card} tabIndex={-1} {...pop} transition={springs.snappy} className={CARD_SURFACE}>
            <CardBody
              member={member}
              onMention={undefined}
              onMessage={() => {
                onLeave();
                api.openDm([member.id]).then((id) => navigate({ kind: "channel", id }), reportError);
              }}
              onViewInMembers={() => {
                onLeave();
                openSettings({ section: "members", membersTab: "members" });
              }}
            />
          </motion.div>
        </RadixPopover.Content>
      </RadixPopover.Portal>
    </RadixPopover.Root>
  );
}
