import { Hash, Lock, MessageCircle } from "lucide-react";
import type { ChannelKind } from "../../lib/api";

/** A channel's icon: # for public, a lock for private, a speech bubble for a DM. */
export function ChannelGlyph({ kind, size = 15, className }: { kind: ChannelKind; size?: number; className?: string }) {
  if (kind === "private") return <Lock size={size} className={className} aria-label="Private" />;
  if (kind === "dm") return <MessageCircle size={size} className={className} aria-label="Direct message" />;
  return <Hash size={size} className={className} />;
}
