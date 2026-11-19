import { Bot } from "lucide-react";
import { initials, memberHue } from "../lib/people";

export function Avatar({ id, name, isBot = false, size = 32 }: { id: string; name: string; isBot?: boolean; size?: number }) {
  const hue = memberHue(id);
  return (
    <div
      className="relative grid shrink-0 place-items-center rounded-[30%] font-semibold text-white"
      style={{
        width: size,
        height: size,
        fontSize: size * 0.38,
        background: `linear-gradient(140deg, hsl(${hue} 70% 62%), hsl(${hue + 24} 62% 48%))`,
      }}
      aria-hidden="true"
    >
      {isBot ? <Bot size={size * 0.55} strokeWidth={2.2} /> : initials(name)}
    </div>
  );
}
