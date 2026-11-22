import { useCallback } from "react";
import { api, type Huddle } from "../../lib/api";
import { displayName } from "../../lib/people";
import { useLive } from "../../lib/useLive";
import { useWorkspace } from "../../lib/workspace";

/** Who's in `channel`'s huddle, kept current; undefined until loaded. */
export function useHuddle(channel: string): Huddle | undefined {
  const load = useCallback(() => api.huddle(channel), [channel]);
  return useLive(load, (change) => change.kind === "huddle" && change.channel === channel);
}

/** A member's name, for their avatar. */
export function useMemberNames(): (member: string) => string {
  const { members } = useWorkspace();
  return useCallback(
    (member: string) => displayName(members?.find((candidate) => candidate.id === member)?.name ?? "", member),
    [members],
  );
}

/** A member's name as huddle text says it: "You" for you. */
export function useHuddleNames(): (member: string) => string {
  const { session } = useWorkspace();
  const nameOf = useMemberNames();
  return useCallback((member: string) => (member === session.me ? "You" : nameOf(member)), [session.me, nameOf]);
}
