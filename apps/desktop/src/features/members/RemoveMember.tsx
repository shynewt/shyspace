import { UserMinus } from "lucide-react";
import { useState } from "react";
import { api, type Member } from "../../lib/api";
import { useMembers } from "../../lib/members";
import { distinctName } from "../../lib/people";
import { showToast } from "../../lib/toast";
import { useWorkspace } from "../../lib/workspace";
import { ConfirmDialog, IconButton } from "../../ui";

/** The admin's Remove button for a member, asking first: a removal can't be undone. */
export function RemoveMemberButton({ member }: { member: Member }) {
  const { session } = useWorkspace();
  const [isConfirming, setConfirming] = useState(false);
  const members = useMembers();
  const name = distinctName(member, members ?? []);
  return (
    <>
      <IconButton label={`Remove ${name}`} icon={<UserMinus size={15} />} onClick={() => setConfirming(true)} />
      <ConfirmDialog
        open={isConfirming}
        onOpenChange={setConfirming}
        title={`Remove ${name} from ${session.workspace_name}?`}
        description="They keep what they already synced, but can't read anything new: new messages and pages use keys they never get. This can't be undone."
        confirmLabel="Remove"
        onConfirm={async () => {
          await api.removeMember(member.id);
          showToast(`${name} was removed.`);
        }}
      />
    </>
  );
}
