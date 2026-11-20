import { Hash, Lock, MessageCircle, NotebookPen } from "lucide-react";
import type { ChannelKind } from "../../lib/api";

/** A channel's icon: # for public, a lock for private, a speech bubble for a DM, a notebook for notes. */
export function ChannelGlyph({ kind, size = 15, className }: { kind: ChannelKind; size?: number; className?: string }) {
  if (kind === "private") return <Lock size={size} className={className} aria-label="Private" />;
  if (kind === "dm") return <MessageCircle size={size} className={className} aria-label="Direct message" />;
  if (kind === "notes") return <NotebookPen size={size} className={className} aria-label="Notes" />;
  return <Hash size={size} className={className} />;
}
