import { useMembers } from "../../lib/members";
import { displayName } from "../../lib/people";
import { cx } from "../../ui";
import { MemberAvatar } from "../members/MemberAvatar";

/**
 * A thread's latest repliers as small overlapping avatars, the newest on
 * top. `ringClassName` draws the gap between them in the background's
 * colour, also on hover.
 */
export function ReplierAvatars({ authorIds, ringClassName }: { authorIds: string[]; ringClassName: string }) {
  const members = useMembers();
  if (authorIds.length === 0) return null;
  return (
    <span className="flex shrink-0 -space-x-1">
      {authorIds.map((id, index) => {
        const member = members?.find((candidate) => candidate.id === id);
        return (
          <span
            key={id}
            style={{ zIndex: authorIds.length - index }}
            className={cx("relative rounded-[30%] ring-2 transition-colors", ringClassName)}
          >
            <MemberAvatar
              id={id}
              name={displayName(member?.name ?? "", id)}
              isBot={member?.is_bot ?? false}
              size={16}
            />
          </span>
        );
      })}
    </span>
  );
}
