import { type ReactNode, useCallback } from "react";
import { api } from "../../lib/api";
import { displayName } from "../../lib/people";
import { useLive } from "../../lib/useLive";

/** One setting: a title and a line of help above its control. */
export function Setting({
  title,
  description,
  children,
}: {
  title: string;
  description?: ReactNode;
  children: ReactNode;
}) {
  return (
    <div className="flex flex-col gap-2 border-b border-line py-4 first:pt-0 last:border-b-0">
      <div>
        <div className="text-[13px] font-medium">{title}</div>
        {description && <p className="mt-0.5 text-[12px] leading-relaxed text-muted">{description}</p>}
      </div>
      {children}
    </div>
  );
}

/** The workspace admin's name, once the members have loaded. */
export function useAdminName(): string | undefined {
  const loadMembers = useCallback(() => api.members(), []);
  const members = useLive(loadMembers, (change) => change.kind === "members");
  if (!members) return undefined;
  const admin = members.find((member) => member.is_admin && member.active);
  return admin ? displayName(admin.name, admin.id) : "the workspace admin";
}

/** What a member who isn't the admin sees instead of the admin's controls. */
export function ManagedByAdmin({ children }: { children?: ReactNode }) {
  const adminName = useAdminName();
  if (!adminName) return null;
  return (
    <div className="flex flex-col gap-1 text-[13px]">
      <p>Managed by {adminName}.</p>
      {children && <p className="text-[12px] leading-relaxed text-muted">{children}</p>}
    </div>
  );
}
