/**
 * The workspace's members, loaded once by the shell for the whole app.
 * Statuses past their end time are dropped here, live: nothing announces
 * that a status ended, so a timer wakes up when the next one does.
 */
import { useEffect, useMemo, useState } from "react";
import type { Member, MemberStatus } from "./api";
import { useWorkspace } from "./workspace";

/** setTimeout can't wait longer than about 24 days. */
const MAX_TIMER_MS = 2 ** 31 - 1;

/** The status if it hasn't ended by `nowMs`. */
export function currentStatus(status: MemberStatus | null, nowMs: number): MemberStatus | null {
  if (!status || (status.until_ms !== null && status.until_ms <= nowMs)) return null;
  return status;
}

/** `members` with ended statuses removed, updated the moment the next one ends. */
export function useMembersWithCurrentStatus(members: Member[] | undefined): Member[] | undefined {
  const [nowMs, setNowMs] = useState(Date.now);
  useEffect(() => {
    const now = Date.now();
    const ends = (members ?? []).flatMap((member) => {
      const until = member.status?.until_ms;
      return until !== undefined && until !== null && until > nowMs ? [until] : [];
    });
    if (ends.length === 0) return;
    const nextEnd = Math.min(...ends);
    // `nowMs` is when the clock last ticked, which may be long ago: measure from now.
    if (nextEnd <= now) {
      setNowMs(now);
      return;
    }
    const timer = setTimeout(() => setNowMs(Date.now()), Math.min(nextEnd - now, MAX_TIMER_MS));
    return () => clearTimeout(timer);
  }, [members, nowMs]);
  return useMemo(
    () => members?.map((member) => ({ ...member, status: currentStatus(member.status, nowMs) })),
    [members, nowMs],
  );
}

/** Everyone in the workspace (former members too); undefined until loaded. */
export function useMembers(): Member[] | undefined {
  return useWorkspace().members;
}

export function useMember(id: string): Member | undefined {
  return useMembers()?.find((member) => member.id === id);
}
