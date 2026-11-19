import { Bot } from "lucide-react";

const HUES = [262, 200, 152, 32, 340, 12, 288, 176];

/** A stable, pleasant hue per member, derived from their id. */
function hueFor(id: string) {
  let hash = 0;
  for (let index = 0; index < id.length; index++) hash = (hash * 31 + id.charCodeAt(index)) | 0;
  return HUES[Math.abs(hash) % HUES.length];
}

function initials(name: string) {
  const words = name.trim().split(/\s+/).filter(Boolean);
  if (words.length === 0) return "?";
  return (words[0][0] + (words[1]?.[0] ?? "")).toUpperCase();
}

export function Avatar({
  id,
  name,
  isBot = false,
  size = 32,
}: {
  id: string;
  name: string;
  isBot?: boolean;
  size?: number;
}) {
  const hue = hueFor(id);
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
    </div>
  );
}
