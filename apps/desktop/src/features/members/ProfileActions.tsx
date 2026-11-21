import {
  AtSign,
  Bot,
  Copy,
  IdCard,
  MessageCircle,
  MoreHorizontal,
  Pencil,
  Search,
  Smile,
  Users,
  X,
} from "lucide-react";
import { api, type Member } from "../../lib/api";
import { displayName } from "../../lib/people";
import type { ProfileField } from "../../lib/settings";
import { reportError, showToast } from "../../lib/toast";
import { useWorkspace } from "../../lib/workspace";
import { Button, Menu, type MenuItem } from "../../ui";

const ICON = 13;
const MENU_ICON = 14;

/** Where "Edit profile" opens: the first key field still empty, else the name. */
function fieldToEdit(member: Member): ProfileField {
  const { title, pronouns, time_zone } = member.details;
  if (!title) return "title";
  if (!pronouns) return "pronouns";
  if (!time_zone) return "time_zone";
  return "name";
}

/**
 * What you can do from someone's profile: message or mention them, and more
 * in a menu. Your own offers your status and editing your profile instead.
 * Actions that take you elsewhere call `onLeave` first, which closes the card
 * or dialog with focus moving on.
 */
export function ProfileActions({
  member,
  onLeave,
  onMention,
  onViewFullProfile,
}: {
  member: Member;
  onLeave: () => void;
  /** Absent where there's no message box to mention them in. */
  onMention: (() => void) | undefined;
  /** Absent in the full profile itself. */
  onViewFullProfile: (() => void) | undefined;
}) {
  const { session, navigate, openSettings, openSearch, openStatus } = useWorkspace();
  const name = displayName(member.name, member.id);
  const isMe = member.id === session.me;
  const canMention = onMention !== undefined && member.active && !isMe && member.name !== "";
  const leaveFor = (action: () => void) => () => {
    onLeave();
    action();
  };

  const fullProfile: MenuItem[] = onViewFullProfile
    ? [{ label: "View full profile", icon: <IdCard size={MENU_ICON} />, onSelect: onViewFullProfile }]
    : [];
  const viewInMembers: MenuItem = {
    label: "View in members",
    icon: <Users size={MENU_ICON} />,
    onSelect: leaveFor(() => openSettings({ section: "members", membersTab: "members" })),
  };
  const viewMessages: MenuItem = {
    label: member.is_bot ? "View its messages" : "View their messages",
    icon: <Search size={MENU_ICON} />,
    onSelect: leaveFor(() => openSearch(`from:"${name.replaceAll('"', "")}"`)),
  };
  const copyName: MenuItem = {
    label: "Copy name",
    icon: <Copy size={MENU_ICON} />,
    onSelect: () => navigator.clipboard.writeText(name).then(() => showToast("Name copied."), reportError),
  };
  const agentSettings: MenuItem[] =
    member.is_bot && session.is_admin
      ? [
          {
            label: "Agent settings",
            icon: <Bot size={MENU_ICON} />,
            onSelect: leaveFor(() => openSettings({ section: "agents" })),
          },
        ]
      : [];
  const editStatus: MenuItem[] = member.status
    ? [{ label: "Edit status", icon: <Smile size={MENU_ICON} />, onSelect: leaveFor(openStatus) }]
    : [];
  const menuItems: MenuItem[] = isMe
    ? [...fullProfile, ...editStatus, viewInMembers]
    : [...fullProfile, copyName, viewMessages, ...agentSettings, viewInMembers];

  return (
    <div className="flex items-center gap-2">
      {isMe ? (
        <>
          {member.status ? (
            <Button
              size="sm"
              icon={<X size={ICON} />}
              onClick={() => api.clearStatus().then(() => showToast("Status cleared."), reportError)}
            >
              Clear status
            </Button>
          ) : (
            <Button size="sm" variant="primary" icon={<Smile size={ICON} />} onClick={leaveFor(openStatus)}>
              Set status
            </Button>
          )}
          <Button
            size="sm"
            icon={<Pencil size={ICON} />}
            onClick={leaveFor(() => openSettings({ section: "profile", field: fieldToEdit(member) }))}
          >
            Edit profile
          </Button>
        </>
      ) : (
        member.active && (
          <Button
            size="sm"
            variant="primary"
            icon={<MessageCircle size={ICON} />}
            onClick={leaveFor(() =>
              api.openDm([member.id]).then((id) => navigate({ kind: "channel", id }), reportError),
            )}
          >
            Message
          </Button>
        )
      )}
      {canMention && (
        <Button size="sm" icon={<AtSign size={ICON} />} onClick={onMention}>
          Mention
        </Button>
      )}
      <Menu
        items={menuItems}
        trigger={
          <Button size="sm" aria-label="More actions" icon={<MoreHorizontal size={ICON} />} className="ml-auto px-2" />
        }
      />
    </div>
  );
}
