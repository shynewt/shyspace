import { Tabs } from "radix-ui";
import type { ReactNode } from "react";
import { useMembers } from "../../lib/members";
import type { MembersTab } from "../../lib/settings";
import { useWorkspace } from "../../lib/workspace";
import { cx, SectionLabel } from "../../ui";
import { CreateInvite, OpenInvites } from "../members/Invites";
import { JoinRequestList, WaitingRequestsBadge } from "../members/JoinRequests";
import { MemberRows, SearchableMemberList } from "../members/MemberList";
import { RemoveMemberButton } from "../members/RemoveMember";

const TABS: MembersTab[] = ["members", "invites", "requests"];

/** Everyone in the workspace; the admin also gets invites and join requests, as tabs. */
export function MembersSettings({ tab, onTabChange }: { tab: MembersTab; onTabChange: (tab: MembersTab) => void }) {
  const { session } = useWorkspace();
  if (!session.is_admin) return <MemberDirectory />;
  return (
    <Tabs.Root
      value={tab}
      onValueChange={(value) => {
        const chosen = TABS.find((candidate) => candidate === value);
        if (chosen) onTabChange(chosen);
      }}
    >
      <Tabs.List aria-label="Members" className="mb-4 flex gap-5 border-b border-line">
        <TabTrigger value="members" isActive={tab === "members"}>
          Members
        </TabTrigger>
        <TabTrigger value="invites" isActive={tab === "invites"}>
          Invites
        </TabTrigger>
        <TabTrigger value="requests" isActive={tab === "requests"}>
          Requests <WaitingRequestsBadge />
        </TabTrigger>
      </Tabs.List>
      <Tabs.Content value="members">
        <MemberDirectory />
      </Tabs.Content>
      <Tabs.Content value="invites" className="flex flex-col gap-5">
        <CreateInvite />
        <OpenInvites />
      </Tabs.Content>
      <Tabs.Content value="requests">
        <JoinRequestList />
      </Tabs.Content>
    </Tabs.Root>
  );
}

function TabTrigger({ value, isActive, children }: { value: MembersTab; isActive: boolean; children: ReactNode }) {
  return (
    <Tabs.Trigger
      value={value}
      className={cx(
        "-mb-px flex h-9 items-center gap-1.5 rounded-t-md border-b-2 text-[13px] -outline-offset-2 transition-colors",
        isActive ? "border-accent font-medium text-ink" : "border-transparent text-muted hover:text-ink",
      )}
    >
      {children}
    </Tabs.Trigger>
  );
}

function MemberDirectory() {
  const { session } = useWorkspace();
  const members = useMembers();
  if (!members) return null;
  const active = members.filter((member) => member.active);
  const former = members.filter((member) => !member.active);
  const canRemove = (member: (typeof members)[number]) =>
    session.is_admin && !member.is_admin && member.id !== session.me;
  return (
    <div className="flex flex-col gap-5">
      <SearchableMemberList
        label="Members"
        members={active}
        actions={(member) => canRemove(member) && <RemoveMemberButton member={member} />}
      />
      {former.length > 0 && (
        <div>
          <SectionLabel>Former members</SectionLabel>
          <MemberRows label="Former members" members={former} />
        </div>
      )}
    </div>
  );
}
