import { Bot, Info, Lock } from "lucide-react";
import type { ReactNode } from "react";
import { useMembers } from "../../lib/members";
import { IconButton, Popover } from "../../ui";
import { HeaderAction } from "../shell/ViewHeader";

type InfoItem = { id: string; icon: ReactNode; label: string; detail: string; look: "pill" | "accent-pill" };

/**
 * What's worth knowing about a channel, at the end of its header: pills that
 * open their details, icons alone when the header is narrow, and one Info
 * button when it's narrower still.
 */
export function ChannelInfo() {
  const members = useMembers();
  const agents = members?.filter((member) => member.active && member.is_bot).length ?? 0;
  const items: InfoItem[] = [
    ...(agents > 0
      ? [
          {
            id: "agents",
            icon: <Bot size={12} />,
            label: `${agents} ${agents === 1 ? "agent" : "agents"} can read this`,
            detail: "AI agents are members and can read this channel. What they read goes to their AI provider.",
            look: "accent-pill" as const,
          },
        ]
      : []),
    {
      id: "encrypted",
      icon: <Lock size={11} />,
      label: "End-to-end encrypted",
      detail:
        "Everything you write is encrypted on this device before it leaves. Only current members have the keys: the storage bucket and the network see scrambled data.",
      look: "pill",
    },
  ];
  return (
    <div className="ml-auto flex shrink-0 items-center gap-1.5">
      {items.map((item) => (
        <div key={item.id} className="flex @max-header-pills/header:hidden">
          <Popover side="bottom" trigger={<HeaderAction look={item.look} icon={item.icon} label={item.label} />}>
            <InfoDetails items={[item]} />
          </Popover>
        </div>
      ))}
      <div className="flex @header-pills/header:hidden">
        <Popover side="bottom" trigger={<IconButton label="Channel info" icon={<Info size={15} />} />}>
          <InfoDetails items={items} />
        </Popover>
      </div>
    </div>
  );
}

function InfoDetails({ items }: { items: InfoItem[] }) {
  return (
    <ul className="flex w-72 max-w-[calc(100vw-2rem)] flex-col gap-3 p-2.5">
      {items.map((item) => (
        <li key={item.id} className="flex gap-2.5">
          <span className="mt-0.5 text-decorative">{item.icon}</span>
          <span className="min-w-0 text-sm">
            <span className="block font-medium">{item.label}</span>
            <span className="mt-0.5 block leading-relaxed text-muted">{item.detail}</span>
          </span>
        </li>
      ))}
    </ul>
  );
}
