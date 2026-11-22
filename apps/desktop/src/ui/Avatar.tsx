import { Bot } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import { cx } from "./cx";
import { springs } from "./motion";

const HUES = [262, 200, 152, 32, 340, 12, 288, 176];

/** A stable, pleasant hue per member, derived from their id. */
export function avatarHue(id: string) {
  let hash = 0;
  for (let index = 0; index < id.length; index++) hash = (hash * 31 + id.charCodeAt(index)) | 0;
  return HUES[Math.abs(hash) % HUES.length];
}

export function initials(name: string) {
  const words = name.trim().split(/\s+/).filter(Boolean);
  if (words.length === 0) return "?";
  return (words[0][0] + (words[1]?.[0] ?? "")).toUpperCase();
}

export function Avatar({
  id,
  name,
  isBot = false,
  size = 32,
  presence,
}: {
  id: string;
  name: string;
  isBot?: boolean;
  size?: number;
  /** A small dot: green online, an amber ring when away, none when unreachable. */
  presence?: "online" | "away" | "unreachable";
}) {
  const hue = avatarHue(id);
  return (
    <div
      aria-hidden="true"
      className="relative grid shrink-0 place-items-center rounded-[30%] font-semibold text-white"
      style={{
        width: size,
        height: size,
        fontSize: size * 0.38,
        background: `linear-gradient(140deg, hsl(${hue} 70% 62%), hsl(${hue + 24} 62% 48%))`,
      }}
    >
      {isBot ? <Bot size={size * 0.55} strokeWidth={2.2} /> : initials(name)}
      <AnimatePresence initial={false}>
        {(presence === "online" || presence === "away") && (
          <motion.span
            initial={{ scale: 0.4, opacity: 0 }}
            animate={{ scale: 1, opacity: 1 }}
            exit={{ scale: 0.4, opacity: 0 }}
            transition={springs.pop}
            className={cx(
              "absolute -right-0.5 -bottom-0.5 rounded-full border-2 border-surface transition-colors duration-base",
              presence === "online" ? "bg-success" : "bg-surface ring-2 ring-warning ring-inset",
            )}
            style={{ width: Math.max(9, size * 0.32), height: Math.max(9, size * 0.32) }}
          />
        )}
      </AnimatePresence>
    </div>
  );
}
