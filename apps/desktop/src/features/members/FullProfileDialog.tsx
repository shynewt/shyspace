import { useMember } from "../../lib/members";
import { displayName } from "../../lib/people";
import { useWorkspace } from "../../lib/workspace";
import { Dialog, DialogActions } from "../../ui";
import { ProfileActions } from "./ProfileActions";
import { Bio, DetailRows, ProfileHeader, StatusBox, WorkingHoursRow } from "./ProfileParts";

/** Everything a member shares, the bio in full, and the card's actions. Not a step in history. */
export function FullProfileDialog({
  memberId,
  returnFocusTo,
  onClose,
}: {
  /** Who to show; null while closed. */
  memberId: string | null;
  returnFocusTo: HTMLElement | null;
  onClose: () => void;
}) {
  const member = useMember(memberId ?? "");
  const { session } = useWorkspace();
  const name = member ? displayName(member.name, member.id) : "";
  return (
    <Dialog
      open={memberId !== null && member !== undefined}
      onOpenChange={(open) => !open && onClose()}
      title={`${name}'s full profile`}
      hideTitle
      width={460}
      hasCloseButton
      returnFocusTo={returnFocusTo}
    >
      {member && (
        <>
          <div className="pr-8">
            <ProfileHeader member={member} isMe={member.id === session.me} />
          </div>
          <StatusBox member={member} />
          <DetailRows member={member}>{member.details.working_hours && <WorkingHoursRow member={member} />}</DetailRows>
          {member.details.bio && <Bio text={member.details.bio} />}
          <DialogActions>
            <ProfileActions member={member} onLeave={onClose} onMention={undefined} onViewFullProfile={undefined} />
          </DialogActions>
        </>
      )}
    </Dialog>
  );
}
