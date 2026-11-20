import { Plus } from "lucide-react";
import { useState } from "react";
import { api, type Channel } from "../../lib/api";
import { channelTitle } from "../../lib/channels";
import { useLive } from "../../lib/useLive";
import { useWorkspace } from "../../lib/workspace";
import { Avatar, IconButton } from "../../ui";
import { NewDmDialog } from "../private/NewPrivateDialogs";
import { NavItem } from "./NavItem";

/** Your DMs, with a button to start one. */
export function DirectMessages() {
  const { channels } = useWorkspace();
  const members = useLive(api.members, (change) => change.kind === "members");
  const [isStarting, setStarting] = useState(false);
  const dms = channels.filter((channel) => channel.kind === "dm");
  return (
    <section aria-label="Direct messages" className="mt-3">
      <div className="group/section flex h-7 items-center gap-0.5 pr-1 pl-2">
        <span className="flex-1 truncate text-[11px] font-semibold tracking-wide text-faint uppercase">
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
        {dms.map((dm) => (
          <DmItem key={dm.id} dm={dm} title={channelTitle(dm, members)} />
        ))}
        {dms.length === 0 && (
          <button
            type="button"
            onClick={() => setStarting(true)}
            className="flex h-8 items-center rounded-lg px-2.5 text-left text-[13px] text-faint hover:bg-surface-2 hover:text-muted"
          >
            Message someone privately
          </button>
        )}
      </div>
      <NewDmDialog open={isStarting} onOpenChange={setStarting} />
    </section>
  );
}

function DmItem({ dm, title }: { dm: Channel; title: string }) {
  const [first] = dm.dm_members;
  return (
    <NavItem
      view={{ kind: "channel", id: dm.id }}
      icon={first ? <Avatar id={first} name={title} size={16} /> : null}
      label={title}
      kind="direct message"
      unread={dm.unread}
      // Every message in a DM is for you, unless you muted it.
      mentions={dm.notify === "nothing" ? 0 : dm.unread}
      isMuted={dm.notify === "nothing"}
    />
  );
}
