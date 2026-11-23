import { NotebookPen, Plus } from "lucide-react";
import { useState } from "react";
import { api, type Channel } from "../../lib/api";
import { channelTitle } from "../../lib/channels";
import { reportError } from "../../lib/toast";
import { useLive } from "../../lib/useLive";
import { useWorkspace } from "../../lib/workspace";
import { IconButton } from "../../ui";
import { useHuddleSummary } from "../huddles/HuddleMarker";
import { MemberAvatar } from "../members/MemberAvatar";
import { describeStatus, shownStatus } from "../members/StatusEmoji";
import { NewDmDialog } from "../private/NewPrivateDialogs";
import { NavItem } from "./NavItem";

/** Your notes, then your DMs, with a button to start one. */
export function DirectMessages() {
  const { channels } = useWorkspace();
  const members = useLive(api.members, (change) => change.kind === "members");
  const [isStarting, setStarting] = useState(false);
  const dms = channels.filter((channel) => channel.kind === "dm");
  return (
    <section aria-label="Direct messages" className="mt-3">
      <div className="group/section flex h-7 items-center gap-0.5 pr-1 pl-2">
        <span className="flex-1 truncate text-2xs font-semibold tracking-wide text-faint uppercase">
          Direct messages
        </span>
        <IconButton
          label="New direct message"
          size="sm"
          icon={<Plus size={14} />}
          onClick={() => setStarting(true)}
          className="opacity-0 group-hover/section:opacity-100 focus-visible:opacity-100"
        />
      </div>
      <div className="flex flex-col gap-px">
        <NotesItem notes={channels.find((channel) => channel.kind === "notes")} />
        {dms.map((dm) => (
          <DmItem key={dm.id} dm={dm} title={channelTitle(dm, members)} />
        ))}
        {dms.length === 0 && (
          <button
            type="button"
            onClick={() => setStarting(true)}
            className="focus-ring-inset flex h-8 items-center rounded-lg px-2.5 text-left text-sm text-faint hover:bg-surface-2 hover:text-muted"
          >
            Message someone privately
          </button>
        )}
      </div>
      <NewDmDialog open={isStarting} onOpenChange={setStarting} />
    </section>
  );
}

/** Always first: your notes, made the first time you open them. */
function NotesItem({ notes }: { notes: Channel | undefined }) {
  const { navigate } = useWorkspace();
  const openForTheFirstTime = () => {
    api
      .openNotes()
      .then((id) => navigate({ kind: "channel", id }))
      .catch(reportError);
  };
  return (
    <NavItem
      view={{ kind: "channel", id: notes?.id ?? "" }}
      onOpen={notes ? undefined : openForTheFirstTime}
      icon={<NotebookPen size={15} />}
      label="Notes"
      kind="only you can see them"
    />
  );
}

function DmItem({ dm, title }: { dm: Channel; title: string }) {
  const { members } = useWorkspace();
  const [first] = dm.dm_members;
  const huddle = useHuddleSummary(dm.id);
  // Only a 1:1 DM has one person whose status to show.
  const person = dm.dm_members.length === 1 ? members?.find((member) => member.id === first) : undefined;
  const status = person && shownStatus(person);
  return (
    <NavItem
      view={{ kind: "channel", id: dm.id }}
      icon={first ? <MemberAvatar id={first} name={title} size={16} /> : null}
      label={title}
      kind="direct message"
      unread={dm.unread}
      // Every message in a DM is for you, unless you muted it.
      mentions={dm.notify === "nothing" ? 0 : dm.unread}
      isMuted={dm.notify === "nothing"}
      huddle={huddle}
      status={
        status?.emoji
          ? { icon: <span className="text-sm leading-none">{status.emoji}</span>, label: describeStatus(status) }
          : null
      }
    />
  );
}
