import { TriangleAlert } from "lucide-react";
import { cx } from "../../ui";
import { MemberAvatar } from "../members/MemberAvatar";

/**
 * Someone in your huddle: a green ring while they talk, faded while their
 * audio connects, and a warning when their app can't talk with yours.
 */
export function HuddleAvatar({
  id,
  name,
  size,
  isSpeaking,
  isConnecting = false,
  needsUpdate = false,
}: {
  id: string;
  name: string;
  size: number;
  isSpeaking: boolean;
  isConnecting?: boolean;
  needsUpdate?: boolean;
}) {
  return (
    <span className={cx("relative inline-flex transition-opacity", isConnecting && "opacity-45")}>
      <MemberAvatar id={id} name={name} size={size} isSpeaking={isSpeaking} />
      {needsUpdate && (
        <span className="absolute -right-1 -bottom-1 grid size-3.5 place-items-center rounded-full bg-surface">
          <TriangleAlert size={10} className="text-warning" />
        </span>
      )}
    </span>
  );
}
