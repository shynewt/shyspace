import type { ComponentProps } from "react";
import { usePicture } from "../../lib/files";
import { useMember } from "../../lib/members";
import { Avatar } from "../../ui";

/** A member's avatar with their picture, if they shared one. */
export function MemberAvatar(props: Omit<ComponentProps<typeof Avatar>, "pictureUrl">) {
  const member = useMember(props.id);
  const pictureUrl = usePicture(props.id, member?.picture ?? null);
  return <Avatar {...props} pictureUrl={pictureUrl} />;
}
