import { api } from "../../lib/api";
import { ConfirmDialog } from "../../ui";

/** Asks before marking everything read: read positions only move forward, so it can't be undone. */
export function MarkAllReadDialog({ open, onOpenChange }: { open: boolean; onOpenChange: (open: boolean) => void }) {
  return (
    <ConfirmDialog
      open={open}
      onOpenChange={onOpenChange}
      title="Mark everything as read?"
      description="Every channel and every thread you follow counts as read, and the Inbox empties. This can't be undone."
      confirmLabel="Mark all as read"
      onConfirm={() => api.markAllRead()}
    />
  );
}
