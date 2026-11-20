import { Bot } from "lucide-react";
import { useCallback } from "react";
import { api } from "../../lib/api";
import { useLive } from "../../lib/useLive";
import { Tooltip } from "../../ui";

/** Agents can read every channel; say so where people write. */
export function AgentsNote() {
  const loadMembers = useCallback(() => api.members(), []);
  const members = useLive(loadMembers, (change) => change.kind === "members");
  const agents = members?.filter((member) => member.active && member.is_bot).length ?? 0;
  return (
    <span className="ml-auto">
      {agents > 0 && (
        <Tooltip
          label={
            <span className="block max-w-[260px] whitespace-normal">
              AI agents are members and can read this channel. What they read goes to their AI provider.
            </span>
          }
          side="bottom"
        >
          <span className="flex items-center gap-1.5 rounded-full bg-accent-soft px-2.5 py-1 text-xs text-accent-text">
            <Bot size={12} /> {agents} {agents === 1 ? "agent" : "agents"} can read this
          </span>
        </Tooltip>
      )}
    </span>
  );
}
